-- Odak schema: profiles, groups, members, tasks, comments, activity.
-- Every table in public has RLS. Membership checks use security definer helpers in the
-- unexposed private schema, so policies on group_members do not recurse.

create schema if not exists private;
revoke all on schema private from public;
grant usage on schema private to authenticated;

-- ========== profiles (display data from Discord; never used for authorization) ==========
create table public.profiles (
  id uuid primary key references auth.users (id) on delete cascade,
  discord_id text,
  username text not null default '',
  display_name text not null default '',
  avatar_url text,
  updated_at timestamptz not null default now()
);
create index profiles_username_lower_idx on public.profiles (lower(username));

create function private.sync_profile() returns trigger
language plpgsql security definer set search_path = '' as $$
declare
  meta jsonb := coalesce(new.raw_user_meta_data, '{}'::jsonb);
  uname text := coalesce(nullif(meta->>'full_name', ''), split_part(coalesce(meta->>'name', ''), '#', 1));
begin
  insert into public.profiles (id, discord_id, username, display_name, avatar_url, updated_at)
  values (
    new.id,
    meta->>'provider_id',
    uname,
    coalesce(nullif(meta->'custom_claims'->>'global_name', ''), uname),
    coalesce(meta->>'avatar_url', meta->>'picture'),
    now()
  )
  on conflict (id) do update set
    discord_id = excluded.discord_id,
    username = excluded.username,
    display_name = excluded.display_name,
    avatar_url = excluded.avatar_url,
    updated_at = now();
  return new;
end $$;

create trigger on_auth_user_saved
  after insert or update of raw_user_meta_data on auth.users
  for each row execute function private.sync_profile();

-- ========== groups and members ==========
create table public.groups (
  id uuid primary key default gen_random_uuid(),
  name text not null check (char_length(name) between 1 and 80),
  owner_id uuid not null default auth.uid() references auth.users (id) on delete cascade,
  created_at timestamptz not null default now()
);
create index groups_owner_id_idx on public.groups (owner_id);

create table public.group_members (
  group_id uuid not null references public.groups (id) on delete cascade,
  user_id uuid not null references auth.users (id) on delete cascade,
  role text not null default 'member' check (role in ('owner', 'member')),
  joined_at timestamptz not null default now(),
  primary key (group_id, user_id)
);
create index group_members_user_id_idx on public.group_members (user_id);

create function private.is_member(gid uuid) returns boolean
language sql stable security definer set search_path = '' as $$
  select exists (
    select 1 from public.group_members m where m.group_id = gid and m.user_id = (select auth.uid())
  );
$$;

create function private.member_of(gid uuid, uid uuid) returns boolean
language sql stable security definer set search_path = '' as $$
  select (select auth.uid()) is not null and exists (
    select 1 from public.group_members m where m.group_id = gid and m.user_id = uid
  );
$$;

create function private.is_owner(gid uuid) returns boolean
language sql stable security definer set search_path = '' as $$
  select exists (select 1 from public.groups g where g.id = gid and g.owner_id = (select auth.uid()));
$$;

create function private.shares_group(other uuid) returns boolean
language sql stable security definer set search_path = '' as $$
  select exists (
    select 1
    from public.group_members a
    join public.group_members b on b.group_id = a.group_id
    where a.user_id = (select auth.uid()) and b.user_id = other
  );
$$;

create function private.add_owner_membership() returns trigger
language plpgsql security definer set search_path = '' as $$
begin
  insert into public.group_members (group_id, user_id, role) values (new.id, new.owner_id, 'owner');
  return new;
end $$;

create trigger on_group_created
  after insert on public.groups
  for each row execute function private.add_owner_membership();

-- ========== tasks ==========
create table public.tasks (
  id uuid primary key,
  owner_id uuid not null default auth.uid() references auth.users (id) on delete cascade,
  group_id uuid references public.groups (id) on delete cascade,
  assignee_id uuid references auth.users (id) on delete set null,
  title text not null check (char_length(title) between 1 and 500),
  description text not null default '',
  priority text not null default 'low' check (priority in ('high', 'medium', 'low')),
  status text not null default 'pending' check (status in ('pending', 'in_progress', 'completed')),
  tags text[] not null default '{}',
  subtasks jsonb not null default '[]',
  due_date timestamptz,
  reminder jsonb,
  "order" bigint not null default 0,
  created_at timestamptz not null default now(),
  -- set by the client; the sync uses it for last-write-wins, so the server never rewrites it
  updated_at timestamptz not null default now()
);
create index tasks_owner_id_idx on public.tasks (owner_id);
create index tasks_group_id_idx on public.tasks (group_id);
create index tasks_assignee_id_idx on public.tasks (assignee_id);

create function private.can_see_task(tid uuid) returns boolean
language sql stable security definer set search_path = '' as $$
  select exists (
    select 1 from public.tasks t
    where t.id = tid
      and (t.owner_id = (select auth.uid())
           or (t.group_id is not null and exists (
                 select 1 from public.group_members m
                 where m.group_id = t.group_id and m.user_id = (select auth.uid()))))
  );
$$;

-- owner_id and created_at never change; only the task owner moves a task between groups
create function private.guard_task_update() returns trigger
language plpgsql set search_path = '' as $$
begin
  if new.owner_id is distinct from old.owner_id then
    raise exception 'owner_id cannot change';
  end if;
  if new.group_id is distinct from old.group_id and old.owner_id is distinct from (select auth.uid())
     and (select auth.uid()) is not null then
    raise exception 'only the task owner can move it between groups';
  end if;
  new.created_at := old.created_at;
  return new;
end $$;

create trigger guard_task_update
  before update on public.tasks
  for each row execute function private.guard_task_update();

-- Last-write-wins upsert used by the sync: writes only when the incoming updated_at is newer.
-- Runs as the caller, so RLS still decides what may be written. Returns false for stale writes.
create function public.upsert_task(task jsonb) returns boolean
language plpgsql security invoker set search_path = '' as $$
declare
  t public.tasks := jsonb_populate_record(null::public.tasks, task);
  written integer;
begin
  insert into public.tasks as cur (
    id, owner_id, group_id, assignee_id, title, description, priority, status,
    tags, subtasks, due_date, reminder, "order", created_at, updated_at)
  values (
    t.id, (select auth.uid()), t.group_id, t.assignee_id, t.title, coalesce(t.description, ''),
    coalesce(t.priority, 'low'), coalesce(t.status, 'pending'), coalesce(t.tags, '{}'),
    coalesce(t.subtasks, '[]'), t.due_date, t.reminder, coalesce(t."order", 0),
    coalesce(t.created_at, now()), coalesce(t.updated_at, now()))
  on conflict (id) do update set
    group_id = excluded.group_id,
    assignee_id = excluded.assignee_id,
    title = excluded.title,
    description = excluded.description,
    priority = excluded.priority,
    status = excluded.status,
    tags = excluded.tags,
    subtasks = excluded.subtasks,
    due_date = excluded.due_date,
    reminder = excluded.reminder,
    "order" = excluded."order",
    updated_at = excluded.updated_at
  where cur.updated_at < excluded.updated_at;
  get diagnostics written = row_count;
  return written > 0;
end $$;

-- ========== comments and activity ==========
create table public.task_comments (
  id uuid primary key default gen_random_uuid(),
  task_id uuid not null references public.tasks (id) on delete cascade,
  user_id uuid not null default auth.uid() references auth.users (id) on delete cascade,
  body text not null check (char_length(body) between 1 and 4000),
  created_at timestamptz not null default now()
);
create index task_comments_task_id_idx on public.task_comments (task_id, created_at);
create index task_comments_user_id_idx on public.task_comments (user_id);

create table public.task_activity (
  id bigint generated always as identity primary key,
  task_id uuid not null references public.tasks (id) on delete cascade,
  user_id uuid references auth.users (id) on delete set null,
  action text not null,
  details text not null default '',
  created_at timestamptz not null default now()
);
create index task_activity_task_id_idx on public.task_activity (task_id, created_at desc);
create index task_activity_user_id_idx on public.task_activity (user_id);

create function private.log_task_activity() returns trigger
language plpgsql security definer set search_path = '' as $$
declare
  actor uuid := (select auth.uid());
begin
  if tg_op = 'INSERT' then
    insert into public.task_activity (task_id, user_id, action, details) values (new.id, actor, 'created', new.title);
    return new;
  end if;
  if new.status is distinct from old.status then
    insert into public.task_activity (task_id, user_id, action, details)
    values (new.id, actor, 'status_changed', old.status || ' → ' || new.status);
  end if;
  if new.assignee_id is distinct from old.assignee_id then
    insert into public.task_activity (task_id, user_id, action, details)
    values (new.id, actor, 'assigned', coalesce(new.assignee_id::text, ''));
  end if;
  if new.title is distinct from old.title then
    insert into public.task_activity (task_id, user_id, action, details) values (new.id, actor, 'title_changed', new.title);
  end if;
  if new.priority is distinct from old.priority then
    insert into public.task_activity (task_id, user_id, action, details)
    values (new.id, actor, 'priority_changed', old.priority || ' → ' || new.priority);
  end if;
  return new;
end $$;

create trigger log_task_activity
  after insert or update on public.tasks
  for each row execute function private.log_task_activity();

create function private.log_comment_activity() returns trigger
language plpgsql security definer set search_path = '' as $$
begin
  insert into public.task_activity (task_id, user_id, action, details)
  values (new.task_id, new.user_id, 'commented', left(new.body, 100));
  return new;
end $$;

create trigger log_comment_activity
  after insert on public.task_comments
  for each row execute function private.log_comment_activity();

-- Member search by Discord username: exact (case-insensitive) match, minimal columns.
-- Security definer because profiles of people you do not share a group with are otherwise hidden.
create function public.find_profile_by_discord_name(name text)
returns table (id uuid, username text, display_name text, avatar_url text)
language sql stable security definer set search_path = '' as $$
  select p.id, p.username, p.display_name, p.avatar_url
  from public.profiles p
  where (select auth.uid()) is not null
    and lower(p.username) = lower(trim(name))
  limit 1;
$$;

-- ========== row level security ==========
alter table public.profiles enable row level security;
alter table public.groups enable row level security;
alter table public.group_members enable row level security;
alter table public.tasks enable row level security;
alter table public.task_comments enable row level security;
alter table public.task_activity enable row level security;

create policy profiles_select on public.profiles for select to authenticated
  using (id = (select auth.uid()) or private.shares_group(id));

create policy groups_select on public.groups for select to authenticated
  using (owner_id = (select auth.uid()) or private.is_member(id));
create policy groups_insert on public.groups for insert to authenticated
  with check (owner_id = (select auth.uid()));
create policy groups_update on public.groups for update to authenticated
  using (owner_id = (select auth.uid()))
  with check (owner_id = (select auth.uid()));
create policy groups_delete on public.groups for delete to authenticated
  using (owner_id = (select auth.uid()));

create policy members_select on public.group_members for select to authenticated
  using (private.is_member(group_id));
create policy members_insert on public.group_members for insert to authenticated
  with check (private.is_owner(group_id) and role = 'member');
create policy members_delete on public.group_members for delete to authenticated
  using ((private.is_owner(group_id) and user_id <> (select auth.uid()))
         or (user_id = (select auth.uid()) and role <> 'owner'));

create policy tasks_select on public.tasks for select to authenticated
  using (owner_id = (select auth.uid()) or (group_id is not null and private.is_member(group_id)));
create policy tasks_insert on public.tasks for insert to authenticated
  with check (owner_id = (select auth.uid())
              and (group_id is null or private.is_member(group_id))
              and (assignee_id is null or (group_id is not null and private.member_of(group_id, assignee_id))));
create policy tasks_update on public.tasks for update to authenticated
  using (owner_id = (select auth.uid()) or (group_id is not null and private.is_member(group_id)))
  with check ((owner_id = (select auth.uid()) or (group_id is not null and private.is_member(group_id)))
              and (assignee_id is null or (group_id is not null and private.member_of(group_id, assignee_id))));
create policy tasks_delete on public.tasks for delete to authenticated
  using (owner_id = (select auth.uid()) or (group_id is not null and private.is_owner(group_id)));

create policy comments_select on public.task_comments for select to authenticated
  using (private.can_see_task(task_id));
create policy comments_insert on public.task_comments for insert to authenticated
  with check (user_id = (select auth.uid()) and private.can_see_task(task_id));
create policy comments_delete on public.task_comments for delete to authenticated
  using (user_id = (select auth.uid()));

create policy activity_select on public.task_activity for select to authenticated
  using (private.can_see_task(task_id));

-- ========== privileges (new tables are not exposed to the Data API by default) ==========
revoke all on public.profiles, public.groups, public.group_members, public.tasks,
  public.task_comments, public.task_activity from anon;
grant select on public.profiles to authenticated;
grant select, insert, update, delete on public.groups to authenticated;
grant select, insert, delete on public.group_members to authenticated;
grant select, insert, update, delete on public.tasks to authenticated;
grant select, insert, delete on public.task_comments to authenticated;
grant select on public.task_activity to authenticated;

revoke execute on all functions in schema private from public, anon;
grant execute on function private.is_member(uuid), private.member_of(uuid, uuid), private.is_owner(uuid),
  private.shares_group(uuid), private.can_see_task(uuid) to authenticated;
revoke execute on function public.upsert_task(jsonb), public.find_profile_by_discord_name(text) from public, anon;
grant execute on function public.upsert_task(jsonb), public.find_profile_by_discord_name(text) to authenticated;

-- ========== realtime ==========
alter publication supabase_realtime add table public.tasks, public.task_comments;
