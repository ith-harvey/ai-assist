#!/usr/bin/env bash
# Seed AI Assist with realistic test data — cards (email/message) + todos
# Usage: ./seed.sh [host] [port]
#   defaults: localhost 8080

set -euo pipefail

HOST="${1:-localhost}"
PORT="${2:-8080}"
BASE="http://${HOST}:${PORT}"

echo "🌱 Seeding AI Assist at ${BASE}..."
echo ""

# ─────────────────────────────────────────────────────────
# CARDS — realistic emails and messages for ith.harvey@gmail.com
# ─────────────────────────────────────────────────────────

echo "📬 Creating approval cards..."

cards=(
  # Email — work (M0)
  '{"sender":"ith.harvey@gmail.com","message":"Hey Ian, the Stellar integration tests are failing on the fee calculation path. Can you take a look before we cut the release?","reply":"On it — I saw the fee rounding issue in the logs. Will push a fix this afternoon.","confidence":0.88,"channel":"email"}'

  # Email — personal
  '{"sender":"ith.harvey@gmail.com","message":"Can you pick up groceries on the way home? We need milk, eggs, and that sourdough from Whole Foods.","reply":"Sure thing, I will swing by Whole Foods after work. Need anything else?","confidence":0.94,"channel":"email"}'

  # Email — GitHub notification
  '{"sender":"ith.harvey@gmail.com","message":"[ith-harvey/ai-assist] PR #67: Rex left a review — 2 comments on the migration logic, requesting changes to the backfill query.","reply":"Thanks Rex, good catches. I will update the backfill to handle NULL payload rows and push a fix.","confidence":0.82,"channel":"email"}'

  # Email — Joey bachelor party
  '{"sender":"ith.harvey@gmail.com","message":"Yo Ian! So for the bachelor party — thinking Nashville March 15-17. Can you check flights from Vegas? Budget around $400 for airfare.","reply":"Nashville sounds perfect. Let me check Southwest and Frontier, I will send you options by tomorrow.","confidence":0.91,"channel":"email"}'

  # Email — newsletter/digest worth surfacing
  '{"sender":"ith.harvey@gmail.com","message":"New episode: Lex Fridman Podcast #428 — Andrej Karpathy on the future of AI agents, tool use, and why transformers are not enough.","reply":"Saving this for my commute. Karpathy on tool use is directly relevant to what we are building.","confidence":0.73,"channel":"email"}'

  # Telegram — friend
  '{"sender":"ith.harvey@gmail.com","message":"Dude did you see the new Ghost in the Shell trailer? SAC_2045 season 3 confirmed","reply":"No way!! Just saw it — the animation style looks way better this time. We gotta watch together.","confidence":0.95,"channel":"telegram"}'

  # Slack — M0 work
  '{"sender":"ith.harvey@gmail.com","message":"@ian heads up — the staging environment is down. Looks like the Stellar Horizon node is unresponsive. Already paged infra.","reply":"Thanks for the heads up. I will hold off deploying until staging is back. Let me know if you need help debugging the Horizon issue.","confidence":0.86,"channel":"slack"}'

  # WhatsApp — Christina
  '{"sender":"ith.harvey@gmail.com","message":"The electrician can come Thursday or Friday. Which works better for you?","reply":"Friday is better — I have back to back meetings Thursday. Morning or afternoon?","confidence":0.93,"channel":"whatsapp"}'

  # Email — film production
  '{"sender":"ith.harvey@gmail.com","message":"Ian, the Atlanta location scout found two more houses for the slasher scenes. Sending photos. Can you review and pick your top choice by Wednesday?","reply":"Perfect, I will review the photos tonight and send my pick with notes on lighting angles.","confidence":0.79,"channel":"email"}'

  # Email — AI/tech newsletter
  '{"sender":"ith.harvey@gmail.com","message":"OpenClaw v2026.2.24 released: Per-agent model overrides, improved session cleanup, and 3 new built-in skills. See changelog for migration notes.","reply":"Nice, the per-agent model overrides are exactly what we needed for Clark. I will upgrade this weekend.","confidence":0.76,"channel":"email"}'
)

for card in "${cards[@]}"; do
  response=$(curl -s -X POST "${BASE}/api/cards/test" \
    -H 'Content-Type: application/json' \
    -d "$card")
  card_id=$(echo "$response" | python3 -c "import sys,json; print(json.load(sys.stdin).get('card_id','?'))" 2>/dev/null || echo "?")
  sender=$(echo "$card" | python3 -c "import sys,json; print(json.load(sys.stdin)['sender'])" 2>/dev/null || echo "?")
  echo "  ✅ Card from ${sender} (${card_id})"
done

echo ""

# ─────────────────────────────────────────────────────────
# TODOS — realistic tasks for Ian
# ─────────────────────────────────────────────────────────

echo "📝 Creating todos..."

TODO_URL="${BASE}/api/todos/test"

# ISO date helpers
tomorrow=$(date -u -v+1d '+%Y-%m-%dT16:00:00Z' 2>/dev/null || date -u -d '+1 day' '+%Y-%m-%dT16:00:00Z')
in_3_days=$(date -u -v+3d '+%Y-%m-%dT18:00:00Z' 2>/dev/null || date -u -d '+3 days' '+%Y-%m-%dT18:00:00Z')
in_5_days=$(date -u -v+5d '+%Y-%m-%dT12:00:00Z' 2>/dev/null || date -u -d '+5 days' '+%Y-%m-%dT12:00:00Z')
in_1_week=$(date -u -v+7d '+%Y-%m-%dT10:00:00Z' 2>/dev/null || date -u -d '+7 days' '+%Y-%m-%dT10:00:00Z')

create_todo() {
  local json="$1"
  local title="$2"
  local response
  response=$(curl -s -w "\n%{http_code}" -X POST "$TODO_URL" \
    -H 'Content-Type: application/json' \
    -d "$json")
  local code
  code=$(echo "$response" | tail -1)
  if [ "$code" = "201" ]; then
    echo "  ✅ Todo: ${title}"
  else
    echo "  ❌ Failed (${code}): ${title}"
  fi
}

create_todo "{\"title\":\"Fix Stellar fee rounding bug\",\"description\":\"Luca flagged failing integration tests on the fee calculation path. Check the rounding logic in the stablecoin transfer module.\",\"todo_type\":\"deliverable\",\"bucket\":\"human_only\",\"priority\":1,\"due_date\":\"${tomorrow}\",\"context\":\"M0 release blocker — needs fix before Thursday cut\"}" \
  "Fix Stellar fee rounding bug"

create_todo "{\"title\":\"Research Nashville flights for Joey's bachelor party\",\"description\":\"Check Southwest and Frontier from LAS to BNA, March 15-17. Budget ~400 for airfare. Send options to Joey.\",\"todo_type\":\"research\",\"bucket\":\"agent_startable\",\"priority\":2,\"due_date\":\"${tomorrow}\",\"context\":\"Joey asked via email — wants response by tomorrow\"}" \
  "Research Nashville flights"

create_todo "{\"title\":\"Pick up groceries from Whole Foods\",\"description\":\"Milk, eggs, sourdough bread. Christina asked.\",\"todo_type\":\"errand\",\"bucket\":\"human_only\",\"priority\":3,\"due_date\":\"${tomorrow}\"}" \
  "Pick up groceries"

create_todo "{\"title\":\"Review Atlanta location scout photos\",\"description\":\"Mike sent two house options for the slasher scenes. Review photos, pick top choice, and send notes on lighting angles.\",\"todo_type\":\"creative\",\"bucket\":\"human_only\",\"priority\":3,\"due_date\":\"${in_3_days}\",\"context\":\"Slasher film production — Atlanta shoot\"}" \
  "Review Atlanta location photos"

create_todo "{\"title\":\"Upgrade OpenClaw to v2026.2.24\",\"description\":\"Per-agent model overrides, session cleanup improvements. Will fix Clark contextTokens mismatch.\",\"todo_type\":\"administrative\",\"bucket\":\"agent_startable\",\"priority\":4,\"due_date\":\"${in_5_days}\",\"context\":\"Changelog has migration notes — read before upgrading\"}" \
  "Upgrade OpenClaw"

create_todo "{\"title\":\"Watch Karpathy on Lex Fridman — tool use and agents\",\"description\":\"Episode 428. Directly relevant to AI Assist architecture. Take notes for Second Brain.\",\"todo_type\":\"learning\",\"bucket\":\"human_only\",\"priority\":5,\"due_date\":\"${in_1_week}\"}" \
  "Watch Karpathy podcast"

create_todo "{\"title\":\"Address Rex PR 67 review comments\",\"description\":\"Two comments on migration backfill query. Handle NULL payload rows edge case.\",\"todo_type\":\"review\",\"bucket\":\"human_only\",\"priority\":2,\"due_date\":\"${tomorrow}\",\"context\":\"GitHub notification — Rex requesting changes\"}" \
  "Address Rex review comments"

create_todo "{\"title\":\"Draft AI Assist onboarding flow copy\",\"description\":\"Write the 5-screen onboarding sequence: account creation, connect services, personality setup, preferences, UI tutorial. Goal: value in under 2 minutes.\",\"todo_type\":\"deliverable\",\"bucket\":\"agent_startable\",\"priority\":3,\"due_date\":\"${in_5_days}\",\"context\":\"From UX brainstorm doc — four silos, two patterns\"}" \
  "Draft onboarding copy"

create_todo "{\"title\":\"Merge PR 64 — todo row styling\",\"description\":\"Card width matching, inline input on expand, removed bottom bar.\",\"todo_type\":\"review\",\"bucket\":\"human_only\",\"status\":\"completed\"}" \
  "Merge PR 64 (completed)"

create_todo "{\"title\":\"Confirm Friday electrician appointment\",\"description\":\"Christina asked — Thursday or Friday. Picked Friday (meetings Thursday). Need to confirm morning or afternoon.\",\"todo_type\":\"administrative\",\"bucket\":\"human_only\",\"priority\":3,\"due_date\":\"${in_3_days}\",\"context\":\"Christina asked via WhatsApp\"}" \
  "Confirm electrician"

echo ""
echo "🌱 Seed complete! ${#cards[@]} cards + 10 todos created."
echo ""
echo "View at:"
echo "  Cards:  ${BASE}/ws/cards (WebSocket)"
echo "  Todos:  ${BASE}/ws/todos (WebSocket)"
echo "  iOS:    Launch the app and check Messages + To-Dos tabs"
