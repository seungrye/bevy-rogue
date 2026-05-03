# 각성 루트 퀘스트 — 전쟁의 서막 / 재생의 불꽃 / 장벽 너머의 그림자

## 전제

프롤로그 `prologue_fog` 완료 후 `flags["character"]` 값에 따라 3개 루트 퀘스트 중 하나가 활성화.
각 루트 퀘스트는 `dormant` 페이즈에서 `FlagIs(character, ...)` auto_advance로 자동 전환.

## 루트 비교

| 루트 | 퀘스트 ID | NPC | 핵심 서사 | 장르 |
|------|----------|-----|-----------|------|
| 스타크 | `stark_quest` | 캣린 | 제이미 라니스터 생포 + 대관식 | 전쟁 서사극 |
| 타르가르옌 | `targaryen_quest` | 조라 | 드래곤 해방 + 드라카리스 발동 | 마법 어드벤처 |
| 나이트워치 | `jon_snow_quest` | 샘웰 | 화이트 워커 탈출 + 이그리트 조우 | 공포/스릴러 |

## stark_quest: 전쟁의 서막

**페이즈 흐름** (9단계):
`dormant` → `war_briefing` → `gather_lords` → `lords_gathered` → `ambush_prep`
→ `ambush_strike` → `duel_jaime` → `jaime_captured` → `coronation_prep` → `king_end`

**핵심 장면:**
- 아버지 처형 소식 — 캣린의 절제된 슬픔
- 매복 개시 — 광역 휘두르기 Log 묘사
- 제이미와 1:1 결투 — 명예로운 일격으로 생포 (죽이지 않음)
- 북부의 왕 대관식

**보상:** `jaime_sword`, `kings_north_crown` / `flags["title"] = "king_in_the_north"`

## targaryen_quest: 재생의 불꽃

**페이즈 흐름** (9단계):
`dormant` → `exile_begins` → `desert_crossing` → `qarth_arrival` → `tower_assault`
→ `tower_inside` → `dragons_freed` → `qarth_plunder` → `conquest_begin`

**핵심 장면:**
- 붉은 사막 횡단 — 드로곤이 길을 인도
- 마법사의 탑 — 횃불로 환영 파훼, 활로 분신 격파
- 드라카리스 첫 발동 — 파로 소각, 드래곤 해방
- 에소스 항로도 확보

**보상:** `warlock_key`(탑 진입), `dragon_chain`(족쇄 해방), `essos_sail_map` / `flags["dracarys_learned"] = "true"`

## jon_snow_quest: 장벽 너머의 그림자

**페이즈 흐름** (9단계):
`dormant` → `beyond_wall` → `fist_of_firstmen` → `wights_attack` → `dragonglass_search`
→ `escape_route` → `rangers_message` → `ygritte_encounter` → `wildling_world`

**핵심 장면:**
- 화이트 워커 기습 — 최초인의 요새 포위
- 고스트 시야 공유 — 포위망 약점 탐색 (Log 묘사)
- 드래곤스톤 화살촉 + 협공 백스테브로 탈출
- 이그리트 조우 — 비폭력 항복 → 야인 세계 잠입

**보상:** `dragonglass_arrows`, `rangers_note`, `ygritte_bow` / `flags["wildling_contact"] = "ygritte"`

## 서사 기법: 전투를 Log로 묘사

전투 기믹(광역 휘두르기, 드라카리스, 협공 백스테브)은 실제 게임 메카닉이 아닌
`Log` 액션 연속으로 묘사. 플레이어는 대화를 통해 서사에 참여하며,
마지막 대사 줄에서 on_interact가 실행되어 행동의 결과가 세계에 반영된다.
