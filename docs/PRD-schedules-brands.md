# PRD — Schedules & Brand Presets

Status: draft
Owner: monk
Scope: ship Schedules first, then Brand preset system, backed by a researched categorized catalog.

---

## 1. Schedules

Goal: profiles can auto-start on a weekly recurring schedule (weekday mask + time window + timezone), with countdown UI and safe interaction with manual sessions and hard mode.

### 1.1 Data model
- [ ] Add `Schedule { days: WeekdayMask, start: HH:MM, end: HH:MM, tz: String, enabled: bool }` in `src/config/mod.rs`
- [ ] Add `schedule: Option<Schedule>` to `Profile`
- [ ] `WeekdayMask` as `u8` bitmask (Mon=1 … Sun=64) with serde as `["mon","tue",...]`
- [ ] TOML round-trip tests
- [ ] Protocol bump? — no, additive field, keep PROTOCOL_VERSION=2

### 1.2 Scheduler loop (daemon)
- [ ] Extend `Supervisor::tick` to scan profiles with `schedule.enabled`
- [ ] Compute next window in profile tz; if `now ∈ window` and no active session → auto-start
- [ ] Auto-started sessions tagged `reason = "scheduled:<profile>"`
- [ ] If a manual session is already active: skip, log `ScheduleSkipped`
- [ ] If window ends: stop session gracefully unless hard mode
- [ ] New `AuditKind::ScheduleFired`, `AuditKind::ScheduleSkipped`
- [ ] Unit test: fake clock + fake profile

### 1.3 Conflict & edge cases
- [ ] Overlapping schedules → first match wins, deterministic by profile name
- [ ] DST: use `chrono-tz` for tz-aware comparisons
- [ ] Crossing midnight window (e.g. 22:00–02:00) supported
- [ ] Hard mode active → schedules still fire but cannot stop session early

### 1.4 IPC
- [ ] `Request::NextScheduled` → `Response::NextScheduled { profile, at }`
- [ ] `ModeDetailPayload` gains `next_run: Option<DateTime>`

### 1.5 TUI
- [ ] Session card: "Next: Work in 3h 23m" line when no active session
- [ ] Mode picker: badge `⏰` on scheduled profiles
- [ ] Confirm screen: show schedule row in Contract panel
- [ ] Mode editor: schedule form (days checkboxes, start/end, tz)
- [ ] Snapshot tests

---

## 2. Brand Preset System

Goal: replace flat hostname lists with a brand-level abstraction so users toggle "Instagram" instead of 6 hostnames, and brands carry app ids + icon + aliases.

### 2.1 Data model
- [ ] `Brand { id, name, category, aliases: Vec<String>, domains: Vec<String>, apps: AppIds, icon: Option<String> }`
- [ ] `AppIds { macos: Vec<String>, windows: Vec<String>, linux: Vec<String>, ios: Vec<String>, android: Vec<String> }`
- [ ] `Profile.brands: Vec<String>` (brand ids) alongside existing `sites`/`apps`
- [ ] Resolution: expand brands → domains + apps at session start (merged with explicit lists)

### 2.2 Registry
- [ ] `assets/brands/global.toml`, `assets/brands/ru.toml`
- [ ] Loader in `src/sites/` (rename to `src/catalog/`)
- [ ] Hot-embed via `include_str!` like current sites
- [ ] Version field per registry for future migrations
- [ ] Tests: load + expand + dedupe

### 2.3 Migration
- [ ] Existing `sites` field preserved; brands are additive
- [ ] Migration helper: map current group names (`@global.social`) → brand ids
- [ ] `monk migrate brands` CLI (optional, one-shot)

### 2.4 TUI
- [ ] Brand picker grid grouped by category with toggle
- [ ] Search/filter
- [ ] Show icon (unicode fallback) + brand name + domain count
- [ ] Confirm screen "Blocked" panel: brand names first, raw hostnames fallback
- [ ] Snapshot tests

---

## 3. Researched Categorized Catalog

Goal: ship a curated, researched list of the most-used distracting brands per category for `global` and `ru` locales. Target ≈15–20 brands per category.

Research method: web search via subagent, cross-reference SimilarWeb / Statista / Alexa top lists, commit with citations in a sibling `.md`.

### 3.1 Global
- [ ] social (facebook, instagram, x/twitter, tiktok, snapchat, threads, linkedin, pinterest, tumblr, bluesky, mastodon, …)
- [ ] video (youtube, netflix, twitch, hulu, disney+, primevideo, hbomax, vimeo, dailymotion, kick, …)
- [ ] news (cnn, bbc, nytimes, guardian, foxnews, reuters, bloomberg, wsj, …)
- [ ] chat (whatsapp, telegram, messenger, discord, slack, teams, signal, …)
- [ ] shopping (amazon, ebay, aliexpress, temu, shein, etsy, walmart, target, …)
- [ ] games (steam, epicgames, roblox, minecraft, leagueoflegends, fortnite, …)
- [ ] adult (curated)
- [ ] ai (chatgpt, claude, gemini, perplexity, midjourney, character.ai, …)
- [ ] productivity_trap (hackernews, reddit, producthunt, medium, substack, …)

### 3.2 RU
- [ ] social (vk, ok, tenchat, …)
- [ ] video (rutube, kinopoisk, ivi, okko, wink, premier, …)
- [ ] news (lenta, rbc, meduza, rt, tass, gazeta, …)
- [ ] chat (tg local, tamtam, …)
- [ ] shopping (wildberries, ozon, yandex market, megamarket, avito, …)
- [ ] games (vkplay, mail.ru games, …)
- [ ] productivity_trap (habr, vc.ru, dtf, tproger, pikabu, …)

### 3.3 QA
- [ ] Each brand has ≥1 domain, canonical name, category
- [ ] No duplicates across categories
- [ ] App ids filled where trivially known (macos bundle id)
- [ ] Lint script in `tests/` validating registry

---

## 4. Out of Scope (parked)

- [ ] Difficulty tiers (Easy / Harder / Hardcore)
- [ ] Passive screen-time sampling (NSWorkspace / xdotool / Win32)
- [ ] Streaks & gamification
- [ ] Git-hook focus gate
- [ ] Calendar sync (gcal busy → auto schedule)
- [ ] Quota / Ceiling block types (Opal parity)

---

## Execution order

1. Section 1 (Schedules) end-to-end
2. Section 3 research → commit registry
3. Section 2 (Brands) data model + TUI
4. Re-evaluate Section 4
