#!/bin/bash
# TDD 강제: git commit 시 .rs 변경에 테스트 변경이 없으면 차단한다

cmd=$(jq -r '.tool_input.command' 2>/dev/null)

# 따옴표 안의 내용을 제거한 뒤 실제 실행 위치에 git commit 이 있는지 확인
stripped=$(echo "$cmd" | sed "s/'[^']*'//g; s/\"[^\"]*\"//g")
echo "$stripped" | grep -qE '(^|[;&|]|\s)\s*git\s+commit' || exit 0

# --no-verify 사용 시 통과
echo "$cmd" | grep -q -- '--no-verify' && exit 0

# 1. 현재 staged .rs 파일
staged_rs=$(git diff --cached --name-only --diff-filter=ACMR 2>/dev/null \
    | grep '\.rs$' | grep -vE '(main|lib)\.rs$')

# 2. 커맨드 내 git add 에 명시된 .rs 파일 (git add A B && git commit 패턴 대응)
cmd_rs=$(echo "$cmd" \
    | grep -oE 'git\s+add\s+[^;&|]+' \
    | sed 's/git[[:space:]]*add[[:space:]]*//' \
    | tr ' ' '\n' \
    | grep '\.rs$' \
    | grep -vE '(main|lib)\.rs$' \
    || true)

all_rs=$(printf '%s\n%s\n' "$staged_rs" "$cmd_rs" | sort -u | grep -v '^$')
[ -z "$all_rs" ] && exit 0

# 3. staged diff에 테스트 변경 있으면 통과
test_in_staged=$(git diff --cached 2>/dev/null | grep -E '^\+[^+]' | grep -cE '#\[test\]|#\[cfg\(test\)\]')
[ "${test_in_staged:-0}" -gt 0 ] && exit 0

# 4. git add 대상 파일들에 테스트 코드가 있는지 확인
for f in $cmd_rs; do
    [ -f "$f" ] || continue
    if git ls-files --error-unmatch "$f" >/dev/null 2>&1; then
        # 기존 파일: HEAD 대비 diff에서 테스트 변경 확인
        test_in_file=$(git diff HEAD -- "$f" 2>/dev/null | grep -E '^\+[^+]' | grep -cE '#\[test\]|#\[cfg\(test\)\]')
        [ "${test_in_file:-0}" -gt 0 ] && exit 0
    else
        # 새 파일: 파일 내용에 테스트 블록이 있으면 통과
        grep -q '#\[cfg(test)\]' "$f" && exit 0
    fi
done

printf '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"TDD 위반: .rs 변경에 테스트 변경이 없습니다. 테스트 추가 후 커밋하거나 --no-verify 사용"}}'
