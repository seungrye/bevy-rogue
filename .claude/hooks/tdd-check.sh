#!/bin/bash
# TDD 강제: git commit 시 .rs 변경에 테스트 변경이 없으면 차단한다

cmd=$(jq -r '.tool_input.command' 2>/dev/null)

# 따옴표 안의 내용을 제거한 뒤 실제 실행 위치에 git commit 이 있는지 확인
stripped=$(echo "$cmd" | sed "s/'[^']*'//g; s/\"[^\"]*\"//g")
echo "$stripped" | grep -qE '(^|[;&|]|\s)\s*git\s+commit' || exit 0

# --no-verify 사용 시 통과
echo "$cmd" | grep -q -- '--no-verify' && exit 0

# git add 와 git commit 을 같은 명령에 함께 쓰면 차단
# (PreToolUse 시점엔 add 가 실행 전이라 staged diff 검사가 불가능하기 때문)
if echo "$stripped" | grep -qE '(^|[;&|]|\s)\s*git\s+add\s'; then
    printf '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"git add 와 git commit 은 반드시 분리해서 실행하세요.\n먼저 git add 를 실행한 뒤, git commit 을 별도 명령으로 실행하세요."}}'
    exit 0
fi

# staged .rs 파일이 없으면 통과
staged_rs=$(git diff --cached --name-only --diff-filter=ACMR 2>/dev/null \
    | grep '\.rs$' | grep -vE '(main|lib)\.rs$')
[ -z "$staged_rs" ] && exit 0

# staged diff에 테스트 변경이 있으면 통과
test_in_staged=$(git diff --cached 2>/dev/null | grep -E '^\+[^+]' | grep -cE '#\[test\]|#\[cfg\(test\)\]')
[ "${test_in_staged:-0}" -gt 0 ] && exit 0

printf '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"TDD 위반: .rs 변경에 테스트 변경이 없습니다. 테스트 추가 후 커밋하거나 --no-verify 사용"}}'
