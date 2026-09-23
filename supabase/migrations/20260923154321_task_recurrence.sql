-- Repeating tasks: the rule travels with the task (the app's own JSON, e.g. {"every":"week","days":[0,3]}).
alter table public.tasks add column recurrence jsonb;

create or replace function public.upsert_task(task jsonb) returns boolean
language plpgsql security invoker set search_path = '' as $$
declare
  t public.tasks := jsonb_populate_record(null::public.tasks, task);
  written integer;
begin
  insert into public.tasks as cur (
    id, owner_id, group_id, assignee_id, title, description, priority, status,
    tags, subtasks, due_date, reminder, "order", created_at, updated_at, recurrence)
  values (
    t.id, (select auth.uid()), t.group_id, t.assignee_id, t.title, coalesce(t.description, ''),
    coalesce(t.priority, 'low'), coalesce(t.status, 'pending'), coalesce(t.tags, '{}'),
    coalesce(t.subtasks, '[]'), t.due_date, t.reminder, coalesce(t."order", 0),
    coalesce(t.created_at, now()), coalesce(t.updated_at, now()), t.recurrence)
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
    updated_at = excluded.updated_at,
    recurrence = excluded.recurrence
  where cur.updated_at < excluded.updated_at;
  get diagnostics written = row_count;
  return written > 0;
end $$;
