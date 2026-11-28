#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    fixture-home)
      printf '%s\n' "$HOME"
      continue
      ;;
    *'"id":"'*)
      id=${line#*'"id":"'}
      id=${id%%'"'*}
      ;;
    *)
      continue
      ;;
  esac
  case "$id" in
    ''|*[!a-f0-9-]*) exit 1 ;;
  esac
  case "$line" in
    *'"method":"initialize"'*)
      result='{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}}'
      ;;
    *'"method":"tools/list"'*)
      result='{"tools":[{"name":"list_issues","inputSchema":{"type":"object"}},{"name":"delete_issue","inputSchema":{"type":"object"}}]}'
      ;;
    *'"method":"tools/call"'*'"name":"list_issues"'*)
      case "$line" in
        *'"query":"fixture-unmatched"'*) id='fixture-unmatched' ;;
        *'"query":"fixture-reverse"'*)
          printf '{"jsonrpc":"2.0","id":"%s","method":"sampling/createMessage"}\n' "$id"
          continue
          ;;
        *'"query":"fixture-malformed"'*)
          printf '{"jsonrpc":"2.0",\n'
          continue
          ;;
      esac
      result=$(printf '{"content":[{"type":"text","text":"fixture-stdio-result %s"}]}' "$HOME")
      ;;
    *)
      exit 1
      ;;
  esac
  printf '{"jsonrpc":"2.0","id":"%s","result":%s}\n' "$id" "$result"
done
