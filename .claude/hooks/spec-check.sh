#!/bin/bash
# Spec-Driven TDD 강제: src/ 의 .rs 파일 생성·수정 전 specs/ 변경이 있어야 한다
# 허용 조건: 직전 커밋이 specs/ 를 포함했거나, 현재 specs/ 에 미커밋 변경이 있는 경우

f=$(jq -r '.tool_input.file_path' 2>/dev/null)

# src/ 하위 .rs 파일만 검사
echo "$f" | grep -qE '/src/.*\.rs$' || exit 0

# mod.rs / main.rs 는 배선 파일 — 제외
echo "$f" | grep -qE '(mod|main)\.rs$' && exit 0

# specs/ 에 직전 커밋 or 미커밋 변경이 있어야 함
# git show --stat 은 커밋 메시지도 포함하므로 diff-tree 로 실제 변경 파일만 확인
last_commit_spec=$(git diff-tree --no-commit-id -r --name-only HEAD 2>/dev/null | grep -c '^specs/')
uncommitted_spec=$(git status --short specs/ 2>/dev/null | grep -c '.')

[ $((last_commit_spec + uncommitted_spec)) -gt 0 ] && exit 0

printf '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"Spec-Driven TDD 위반: 구현 전 specs/ 에 스펙 파일을 먼저 작성·수정하세요"}}'
