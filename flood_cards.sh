#!/usr/bin/env bash
# Flood the card system with 10 realistic multi-channel test cards

HOST="${1:-localhost}"
PORT="${2:-8080}"
URL="http://${HOST}:${PORT}/api/cards/test"

cards=(
  '{"sender":"Sarah","message":"Hey are you still coming tonight?","reply":"Yeah definitely! What time should I be there?","confidence":0.92,"channel":"whatsapp"}'
  '{"sender":"Dev Team","message":"PR #142 needs a review before EOD","reply":"On it, will review in the next hour","confidence":0.87,"channel":"slack"}'
  '{"sender":"Mom","message":"Did you eat today?","reply":"Yes mom, had lunch already :)","confidence":0.95,"channel":"telegram"}'
  '{"sender":"Marcus","message":"Can we reschedule Thursday'\''s meeting?","reply":"Sure, how about Friday at 2pm instead?","confidence":0.78,"channel":"email"}'
  '{"sender":"Alex","message":"The deploy is failing on staging, can you check the logs?","reply":"Looking into it now, seems like a config issue","confidence":0.81,"channel":"slack"}'
  '{"sender":"Jess","message":"Did you see the new episode last night??","reply":"Not yet! No spoilers please, watching tonight","confidence":0.93,"channel":"whatsapp"}'
  '{"sender":"David Chen","message":"Invoice #3847 is overdue, please advise on payment timeline","reply":"Apologies for the delay, processing it today","confidence":0.72,"channel":"email"}'
  '{"sender":"Dad","message":"Can you help me set up the new printer this weekend?","reply":"Of course! I will come by Saturday morning","confidence":0.91,"channel":"telegram"}'
  '{"sender":"Priya","message":"Quick sync on the Q2 roadmap? 15 min today or tomorrow","reply":"Tomorrow works better, how about 10am?","confidence":0.84,"channel":"slack"}'
  '{"sender":"Jamie","message":"Bro you left your jacket at my place","reply":"Oh thanks for letting me know, I will grab it tomorrow","confidence":0.96,"channel":"whatsapp"}'
)

for card in "${cards[@]}"; do
  curl -s -X POST "$URL" \
    -H 'Content-Type: application/json' \
    -d "$card"
  echo
done
