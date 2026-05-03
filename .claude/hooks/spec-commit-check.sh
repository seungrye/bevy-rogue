#!/bin/bash
# Spec-Commit 강제: git commit 시 .rs 변경에 specs/ 변경이 없으면 차단한다
# 허용 조건: staged 또는 같은 명령의 git add 에 specs/ 파일이 포함되어 있을 것

cmd=$(jq -r '.tool_input.command' 2>/dev/null)

# git commit 명령인지 확인
stripped=$(echo "$cmd" | sed "s/'[^']*'//g; s/\"[^\"]*\"//g")
echo "$stripped" | grep -qE '(^|[;&|]|\s)\s*git\s+commit' || exit 0

# --no-verify 사용 시 통과
echo "$cmd" | grep -q -- '--no-verify' && exit 0

# 1. staged .rs 파일 (main.rs / lib.rs 제외 — mod.rs 는 구현 포함이므로 검사 대상)
staged_rs=$(git diff --cached --name-only --diff-filter=ACMR 2>/dev/null \
    | grep '\.rs$' | grep -vE '(main|lib)\.rs$')

# 2. 명령어 내 git add 에 명시된 .rs 파일
cmd_rs=$(echo "$cmd" \
    | grep -oE 'git[[:space:]]+add[[:space:]]+[^;&|]+' \
    | sed 's/git[[:space:]]*add[[:space:]]*//' \
    | tr ' ' '\n' \
    | grep '\.rs$' \
    | grep -vE '(main|lib)\.rs$' \
    || true)

all_rs=$(printf '%s\n%s\n' "$staged_rs" "$cmd_rs" | sort -u | grep -v '^$')

# .rs 변경이 없으면 스펙 불필요
[ -z "$all_rs" ] && exit 0

# 3. staged specs/ 변경 확인
has_staged_spec=$(git diff --cached --name-only --diff-filter=ACMR 2>/dev/null \
    | grep -c '^specs/')

# 4. 명령어 내 git add 에 명시된 specs/ 파일 확인
has_cmd_spec=$(echo "$cmd" \
    | grep -oE 'git[[:space:]]+add[[:space:]]+[^;&|]+' \
    | grep -c 'specs/')

[ $((has_staged_spec + has_cmd_spec)) -gt 0 ] && exit 0

printf '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"Spec-Driven TDD 위반: .rs 변경 시 specs/ 파일도 함께 커밋해야 합니다. 스펙을 추가·수정하세요."}}'
