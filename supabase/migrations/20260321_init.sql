-- Profiles table
create table profiles (
  id uuid references auth.users primary key,
  nickname text not null,
  avatar_url text,
  created_at timestamptz default now()
);

-- Daily snapshots for leaderboard
create table daily_snapshots (
  id uuid primary key default gen_random_uuid(),
  user_id uuid references profiles(id) not null,
  date date not null,
  total_tokens bigint not null,
  cost_usd numeric(10,4) not null,
  messages integer not null,
  sessions integer not null,
  submitted_at timestamptz default now(),
  unique(user_id, date)
);

-- Create index for leaderboard queries
create index idx_snapshots_date on daily_snapshots(date);
create index idx_snapshots_user_date on daily_snapshots(user_id, date);

-- Enable RLS
alter table profiles enable row level security;
alter table daily_snapshots enable row level security;

-- Profiles policies
create policy "profiles_read" on profiles for select using (true);
create policy "profiles_insert" on profiles for insert with check (auth.uid() = id);
create policy "profiles_update" on profiles for update using (auth.uid() = id);

-- Snapshots policies
create policy "snapshots_read" on daily_snapshots for select using (true);
create policy "snapshots_insert" on daily_snapshots for insert with check (auth.uid() = user_id);
create policy "snapshots_update" on daily_snapshots for update using (auth.uid() = user_id);
