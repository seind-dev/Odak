-- Keep the security definer lookup out of the exposed API schema; the public RPC only forwards to it.
create function private.find_profile_by_discord_name(name text)
returns table (id uuid, username text, display_name text, avatar_url text)
language sql stable security definer set search_path = '' as $$
  select p.id, p.username, p.display_name, p.avatar_url
  from public.profiles p
  where (select auth.uid()) is not null
    and lower(p.username) = lower(trim(name))
  limit 1;
$$;
revoke execute on function private.find_profile_by_discord_name(text) from public, anon;
grant execute on function private.find_profile_by_discord_name(text) to authenticated;

create or replace function public.find_profile_by_discord_name(name text)
returns table (id uuid, username text, display_name text, avatar_url text)
language sql stable security invoker set search_path = '' as $$
  select * from private.find_profile_by_discord_name(name);
$$;
