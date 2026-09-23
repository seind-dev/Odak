-- Row level security scenarios for the Odak schema.
-- Runs as one transaction that impersonates test users and always rolls back.
-- Any failed ASSERT aborts with an error naming the scenario.
begin;

insert into auth.users (id, instance_id, aud, role, email, raw_user_meta_data, created_at, updated_at) values
  ('00000000-0000-0000-0000-0000000000a1', '00000000-0000-0000-0000-000000000000', 'authenticated', 'authenticated', 'alice@test.local',
   '{"full_name":"alice","custom_claims":{"global_name":"Alice"},"provider_id":"101","avatar_url":"https://cdn.test/a.png"}', now(), now()),
  ('00000000-0000-0000-0000-0000000000b2', '00000000-0000-0000-0000-000000000000', 'authenticated', 'authenticated', 'bob@test.local',
   '{"full_name":"bob","provider_id":"102"}', now(), now()),
  ('00000000-0000-0000-0000-0000000000c3', '00000000-0000-0000-0000-000000000000', 'authenticated', 'authenticated', 'carol@test.local',
   '{"name":"carol#0","provider_id":"103"}', now(), now());

-- Profiles are filled from Discord metadata, with fallbacks.
do $$ begin
  assert (select username from public.profiles where id = '00000000-0000-0000-0000-0000000000a1') = 'alice', 'profile username from full_name';
  assert (select display_name from public.profiles where id = '00000000-0000-0000-0000-0000000000a1') = 'Alice', 'profile display_name from global_name';
  assert (select display_name from public.profiles where id = '00000000-0000-0000-0000-0000000000b2') = 'bob', 'display_name falls back to username';
  assert (select username from public.profiles where id = '00000000-0000-0000-0000-0000000000c3') = 'carol', 'username falls back to name without #discriminator';
end $$;

-- ===== alice: personal task, group, member, group task =====
set local role authenticated;
select set_config('request.jwt.claims', '{"sub":"00000000-0000-0000-0000-0000000000a1","role":"authenticated"}', true);

insert into public.tasks (id, title, updated_at) values ('10000000-0000-0000-0000-000000000001', 'Alice personal', '2026-01-01T00:00:00Z');
insert into public.groups (id, name) values ('20000000-0000-0000-0000-000000000001', 'Ekip') returning id;  -- RETURNING must pass SELECT
insert into public.group_members (group_id, user_id) values ('20000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-0000000000b2');
insert into public.tasks (id, group_id, assignee_id, title, updated_at)
  values ('10000000-0000-0000-0000-000000000002', '20000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-0000000000b2', 'Group task', '2026-01-01T00:00:00Z');

do $$ begin
  assert (select count(*) from public.group_members where group_id = '20000000-0000-0000-0000-000000000001') = 2, 'owner membership added by trigger + bob';
  assert (select role from public.group_members where user_id = '00000000-0000-0000-0000-0000000000a1') = 'owner', 'creator is owner';
  assert (select count(*) from public.tasks) = 2, 'alice sees both of her tasks';
  assert (select id from public.find_profile_by_discord_name('  BOB ')) = '00000000-0000-0000-0000-0000000000b2', 'lookup by username is trimmed and case-insensitive';
  -- last-write-wins RPC
  assert public.upsert_task('{"id":"10000000-0000-0000-0000-000000000001","title":"stale","updated_at":"2025-12-31T00:00:00Z"}'::jsonb) = false, 'older update is rejected';
  assert (select title from public.tasks where id = '10000000-0000-0000-0000-000000000001') = 'Alice personal', 'stale write left the row alone';
  assert public.upsert_task('{"id":"10000000-0000-0000-0000-000000000001","title":"fresh","updated_at":"2026-01-02T00:00:00Z"}'::jsonb) = true, 'newer update is applied';
  assert public.upsert_task('{"id":"10000000-0000-0000-0000-000000000003","title":"via rpc","updated_at":"2026-01-02T00:00:00Z"}'::jsonb) = true, 'rpc inserts new tasks';
  assert (select owner_id from public.tasks where id = '10000000-0000-0000-0000-000000000003') = '00000000-0000-0000-0000-0000000000a1', 'rpc owner is the caller';
  -- owner cannot remove herself
  delete from public.group_members where user_id = '00000000-0000-0000-0000-0000000000a1';
  assert (select count(*) from public.group_members where user_id = '00000000-0000-0000-0000-0000000000a1') = 1, 'owner cannot leave her own group';
end $$;

-- ===== bob: member =====
select set_config('request.jwt.claims', '{"sub":"00000000-0000-0000-0000-0000000000b2","role":"authenticated"}', true);
do $$ begin
  assert (select count(*) from public.tasks) = 1, 'bob sees only the group task';
  assert (select count(*) from public.profiles) = 2, 'bob sees himself and alice (shared group), not carol';
  update public.tasks set status = 'in_progress', updated_at = '2026-01-03T00:00:00Z' where id = '10000000-0000-0000-0000-000000000002';
  assert (select status from public.tasks where id = '10000000-0000-0000-0000-000000000002') = 'in_progress', 'member can update group task';
  assert public.upsert_task('{"id":"10000000-0000-0000-0000-000000000002","group_id":"20000000-0000-0000-0000-000000000001","assignee_id":"00000000-0000-0000-0000-0000000000b2","title":"Group task (bob)","status":"in_progress","updated_at":"2026-01-04T00:00:00Z"}'::jsonb) = true, 'member can sync a group task through the rpc';
  assert (select owner_id from public.tasks where id = '10000000-0000-0000-0000-000000000002') = '00000000-0000-0000-0000-0000000000a1', 'rpc keeps the owner';
  delete from public.tasks where id = '10000000-0000-0000-0000-000000000002';
  assert (select count(*) from public.tasks where id = '10000000-0000-0000-0000-000000000002') = 1, 'member cannot delete a group task he does not own';
  delete from public.groups where id = '20000000-0000-0000-0000-000000000001';
  assert (select count(*) from public.groups) = 1, 'member cannot delete the group';
  insert into public.task_comments (task_id, body) values ('10000000-0000-0000-0000-000000000002', 'Bakıyorum');
  assert (select count(*) from public.task_activity where task_id = '10000000-0000-0000-0000-000000000002' and action = 'status_changed'
          and user_id = '00000000-0000-0000-0000-0000000000b2') = 1, 'status change logged with the actor';
  assert (select count(*) from public.task_activity where action = 'commented') = 1, 'comment logged';
  begin
    insert into public.group_members (group_id, user_id) values ('20000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-0000000000c3');
    assert false, 'member must not add members';
  exception when insufficient_privilege then null;
  end;
  begin
    update public.tasks set assignee_id = '00000000-0000-0000-0000-0000000000c3' where id = '10000000-0000-0000-0000-000000000002';
    assert false, 'assignee must be a group member';
  exception when insufficient_privilege then null;
  end;
  begin
    update public.tasks set group_id = null where id = '10000000-0000-0000-0000-000000000002';
    assert false, 'only the owner can move a task out of its group';
  exception when others then null;
  end;
end $$;

-- ===== carol: outsider =====
select set_config('request.jwt.claims', '{"sub":"00000000-0000-0000-0000-0000000000c3","role":"authenticated"}', true);
do $$ begin
  assert (select count(*) from public.tasks) = 0, 'outsider sees no tasks';
  assert (select count(*) from public.groups) = 0, 'outsider sees no groups';
  assert (select count(*) from public.task_comments) = 0, 'outsider sees no comments';
  assert (select count(*) from public.task_activity) = 0, 'outsider sees no activity';
  begin
    insert into public.task_comments (task_id, body) values ('10000000-0000-0000-0000-000000000002', 'spam');
    assert false, 'outsider must not comment';
  exception when insufficient_privilege then null;
  end;
  begin
    insert into public.task_activity (task_id, action) values ('10000000-0000-0000-0000-000000000002', 'fake');
    assert false, 'clients must not write activity';
  exception when insufficient_privilege then null;
  end;
end $$;

-- ===== bob leaves, anon sees nothing =====
select set_config('request.jwt.claims', '{"sub":"00000000-0000-0000-0000-0000000000b2","role":"authenticated"}', true);
delete from public.group_members where user_id = '00000000-0000-0000-0000-0000000000b2';
do $$ begin
  assert (select count(*) from public.tasks) = 0, 'after leaving, bob no longer sees the group task';
end $$;

set local role anon;
select set_config('request.jwt.claims', '{"role":"anon"}', true);
do $$ begin
  begin
    perform count(*) from public.tasks;
    assert false, 'anon must have no table access';
  exception when insufficient_privilege then null;
  end;
  begin
    perform public.find_profile_by_discord_name('bob');
    assert false, 'anon must not look up profiles';
  exception when insufficient_privilege then null;
  end;
end $$;

rollback;
