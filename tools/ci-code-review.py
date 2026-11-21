import hashlib
import json
import os
import re
import sys
from pathlib import Path
from urllib.request import HTTPRedirectHandler, Request, build_opener

MAX_JSON = 4 * 1024 * 1024
MAX_PATCH = 512 * 1024
MAX_FILES = 100
MAX_FINDINGS = 30
CATEGORIES = dict(bug='Bug', security='Segurança', performance='Desempenho',
                  maintainability='Manutenção', test='Teste', style='Estilo',
                  documentation='Documentação', other='Outro')
SEVERITIES = dict(critical='Crítica', high='Alta', medium='Média', low='Baixa')
SHA = re.compile(r'[0-9a-f]{40}')
PATH = re.compile(r'[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.-]+)*')
HUNK = re.compile(r'@@ -(\d{1,8})(?:,(\d{1,8}))? \+(\d{1,8})(?:,(\d{1,8}))? @@(?: .*)?')
REVIEW_FIELDS = set('status llm trace_id message summary tool_calls comments groups warnings project_summary '
                    'resume session_id manifest retry_report'.split())
COMMENT_FIELDS = set('path content suggestion_code existing_code start_line end_line thinking category severity'.split())


class Refused(ValueError):
    pass


def require(condition):
    if not condition:
        raise Refused()


def unique_pairs(pairs):
    obj = dict(pairs)
    require(len(pairs) == len(obj))
    return obj


def parse(raw, maximum=MAX_JSON):
    require(0 < len(raw) <= maximum)
    try:
        return json.loads(raw.decode('utf-8'), object_pairs_hook=unique_pairs, parse_constant=lambda _: require(False))
    except (ValueError, RecursionError) as exc:
        raise Refused() from exc


def added_lines(patch, additions, deletions):
    require(isinstance(patch, str) and 0 < len(patch) <= MAX_PATCH)
    added = set()
    new = left = right = removed = 0
    seen = marker = False
    for row in patch.split('\n'):
        match = HUNK.fullmatch(row)
        if match:
            target = int(match[3])
            require(left == right == 0 and (not seen or target >= new))
            left, right = int(match[2] or '1'), int(match[4] or '1')
            require((int(match[1]) > 0 or left == 0) and (target > 0 or right == 0) and left + right > 0)
            new, seen, marker = target, True, False
            continue
        if row == r'\ No newline at end of file':
            require(marker)
            marker = False
            continue
        prefix = row[:1]
        require(seen and prefix in (' ', '+', '-'))
        if prefix != '+':
            require(left > 0)
            left -= 1
            removed += prefix == '-'
        if prefix != '-':
            require(right > 0)
            right -= 1
            if prefix == '+':
                added.add(new)
            new += 1
        marker = True
    require(seen and left == right == 0 and len(added) == additions and removed == deletions)
    return added


def indexed(files):
    require(type(files) is list and len(files) <= MAX_FILES)
    paths = {}
    for entry in files:
        require(type(entry) is dict)
        name, patch = entry.get('filename'), entry.get('patch')
        additions, deletions = entry.get('additions'), entry.get('deletions')
        require(isinstance(name, str) and 0 < len(name) <= 1024 and name not in paths)
        require(patch is None or isinstance(patch, str) and len(patch) <= MAX_PATCH)
        require(all(type(n) is int and 0 <= n <= 10_000_000 for n in (additions, deletions)))
        paths[name] = (patch, additions, deletions)
    entries = sorted(paths.items())
    digest = hashlib.sha256(json.dumps(entries, separators=(',', ':')).encode()).hexdigest()
    return entries, digest


def project(raw, files):
    report = parse(raw)
    require(type(report) is dict and set(report) <= REVIEW_FIELDS)
    require(report.get('status') in ('complete', 'skipped') and report.get('warnings', []) == [])
    findings = report.get('comments')
    if findings is None and 'comments' in report:
        findings = []
    require(type(findings) is list and len(findings) <= MAX_FINDINGS)
    require(report['status'] != 'skipped' or not findings)
    summary = report.get('summary')
    require(summary is None or type(summary) is dict and type(summary.get('comments')) is int
            and summary['comments'] == len(findings) and summary.get('budget_exceeded', False) is False)
    entries, digest = indexed(files)
    indices = {path: i for i, (path, _) in enumerate(entries)}
    items = []
    for finding in findings:
        require(type(finding) is dict and set(finding) <= COMMENT_FIELDS)
        path, category, severity = (finding.get(key) for key in ('path', 'category', 'severity'))
        require(isinstance(path, str) and path in indices)
        require(isinstance(category, str) and category in CATEGORIES)
        require(isinstance(severity, str) and severity in SEVERITIES)
        require(isinstance(finding.get('content'), str))
        require(all(isinstance(finding.get(k, ''), str) for k in ('thinking', 'existing_code', 'suggestion_code')))
        items.append([indices[path], finding.get('start_line'), finding.get('end_line'),
                      list(CATEGORIES).index(category), list(SEVERITIES).index(severity)])
    payload = {'diff': digest, 'items': items}
    render(payload, files, '0' * 40)
    return payload


def render(payload, files, head):
    entries, digest = indexed(files)
    require(type(payload) is dict and set(payload) == {'diff', 'items'} and payload['diff'] == digest)
    items = payload['items']
    require(type(items) is list and len(items) <= MAX_FINDINGS and SHA.fullmatch(head))
    comments, seen, positions = [], set(), {}
    for item in items:
        require(type(item) is list and len(item) == 5 and all(type(v) is int for v in item))
        index, start, end, category, severity = item
        require(0 <= index < len(entries) and 1 <= start <= end <= 10_000_000 and end - start < 30)
        require(0 <= category < len(CATEGORIES) and 0 <= severity < len(SEVERITIES))
        path, patch_data = entries[index]
        require(len(path) <= 240 and PATH.fullmatch(path) and not {'.', '..'}.intersection(path.split('/')))
        if index not in positions:
            positions[index] = added_lines(*patch_data)
        require(all(line in positions[index] for line in range(start, end + 1)))
        require((index, end) not in seen)
        seen.add((index, end))
        label, level = list(CATEGORIES.values())[category], list(SEVERITIES.values())[severity]
        comment = {'path': path, 'line': end, 'side': 'RIGHT',
                   'body': f'Open Code Review sinalizou esta alteração. Categoria: {label}. Severidade: {level}. Verifique manualmente.'}
        if start < end:
            comment.update(start_line=start, start_side='RIGHT')
        comments.append(comment)
    return {'commit_id': head, 'event': 'COMMENT', 'comments': comments,
            'body': f'Open Code Review: {len(comments)} sinalização(ões) para verificação manual. Não é uma aprovação.'}


def snapshot(request, repo, number, base, head):
    require(re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repo) and re.fullmatch(r'[1-9][0-9]{0,8}', str(number)))
    require(SHA.fullmatch(base) and SHA.fullmatch(head))
    path = f'/repos/{repo}/pulls/{number}'

    def check():
        pr = request(path)
        require(pr['number'] == number and pr['state'] == 'open' and pr['draft'] is False)
        require(pr['head']['repo']['full_name'] == repo and pr['base']['repo']['full_name'] == repo)
        require(pr['head']['sha'] == head and pr['base']['sha'] == base and pr['base']['ref'] == 'main')
        count = pr['changed_files']
        require(type(count) is int and 0 <= count <= MAX_FILES)
        return count

    count = check()
    files = request(f'{path}/files?per_page={MAX_FILES}')
    require(type(files) is list and len(files) == count and count == check())
    return path, files


def publish(request, repo, number, base, head, payload):
    path, files = snapshot(request, repo, number, base, head)
    body = render(payload, files, head)
    if body['comments']:
        request(f'{path}/reviews', body)
    return len(body['comments'])


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


def main(mode):
    token = os.environ['GH_TOKEN']
    require(bool(token))
    opener = build_opener(NoRedirect())

    def request(path, body=None):
        req = Request('https://api.github.com' + path,
                      data=None if body is None else json.dumps(body).encode(),
                      headers={'Authorization': 'Bearer ' + token, 'Accept': 'application/vnd.github+json',
                               'X-GitHub-Api-Version': '2022-11-28', 'Content-Type': 'application/json'})
        with opener.open(req, timeout=12) as response:
            if body is None:
                return parse(response.read(MAX_JSON + 1))

    args = (os.environ['GITHUB_REPOSITORY'], int(os.environ['PR_NUMBER']),
            os.environ['BASE_SHA'], os.environ['HEAD_SHA'])
    if mode == 'preflight':
        snapshot(request, *args)
    elif mode == 'project':
        _, files = snapshot(request, *args)
        with (Path(os.environ['RUNNER_TEMP']) / 'mexerica-ocr/result.json').open('rb') as raw:
            payload = project(raw.read(MAX_JSON + 1), files)
        with Path(os.environ['GITHUB_OUTPUT']).open('a', encoding='utf-8') as output:
            print('review=' + json.dumps(payload, separators=(',', ':')), file=output)
    elif mode == 'publish':
        count = publish(request, *args, parse(os.environ['REVIEW'].encode(), 4096))
        print(f'Open Code Review: {count} sinalização(ões) publicada(s).')
    else:
        raise Refused()


if __name__ == '__main__':
    try:
        main(sys.argv[1])
    except Exception:
        sys.exit('Open Code Review: operação recusada ou indisponível.')
