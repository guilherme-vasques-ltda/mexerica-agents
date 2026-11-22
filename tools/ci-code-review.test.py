import importlib.util
import json
import unittest
from contextlib import redirect_stderr, redirect_stdout
from io import StringIO
from pathlib import Path
from tempfile import TemporaryDirectory
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('review', Path(__file__).with_name('ci-code-review.py'))
review = importlib.util.module_from_spec(spec)
spec.loader.exec_module(review)

HEAD = 'a' * 40
BASE = 'b' * 40
FILES = [{'filename': 'src/synthetic.txt', 'patch': '@@ -1,2 +1,3 @@\n same\n+new\n+also new\n-end',
          'additions': 2, 'deletions': 1}]
FINDING = {'path': 'src/synthetic.txt', 'start_line': 2, 'end_line': 3,
           'category': 'bug', 'severity': 'high', 'content': 'O valor vazio interrompe a leitura. Verifique a condição.'}


def result(finding=FINDING, **overrides):
    return json.dumps({'status': 'complete', 'comments': [finding], **overrides}).encode()


class ReviewTest(unittest.TestCase):
    def test_only_validated_structure_crosses_the_boundary(self):
        payload = review.project(result(), FILES)
        self.assertNotIn('PRIVATE-MARKER', json.dumps(payload))
        self.assertNotIn('synthetic.txt', json.dumps(payload))
        rendered = review.render(payload, FILES, HEAD)
        comment, = rendered['comments']
        self.assertEqual((comment['path'], comment['start_line'], comment['line'], comment['side']),
                         ('src/synthetic.txt', 2, 3, 'RIGHT'))
        for forbidden in ('PRIVATE-MARKER', '@reader', 'https:', 'also new'):
            self.assertNotIn(forbidden, json.dumps(rendered))
        self.assertIn(FINDING['content'], comment['body'])

    def test_unsafe_text_never_enters_the_artifact(self):
        for text in ('a @reader', '![](image)', 'domain.example.invalid', 'A <b>x</b>', 'abc\u202Edef',
                     'A `code`', 'A [link]', 'Existe uma sequência inédita nesta frase.', 'sk-test-SyntheticOnly12345',
                     'Bearer exemplo', 'A 192.0.2.1', 'a' * 1501, 'A https://example.invalid', 'x\n# heading'):
            with self.subTest(text=text):
                payload = review.project(result({**FINDING, 'content': text}), FILES, ('sequência inédita',))
                self.assertEqual(payload['items'], [])
        payload = review.project(result({**FINDING, 'suggestion_code': 'x = 3'}), FILES)
        self.assertNotIn('x = 3', json.dumps(payload))
        url = 'https://review.example.invalid/v1'
        payload = review.project(result({**FINDING, 'thinking': url}), FILES, (url,))
        self.assertEqual(payload['items'], [])
        model = 'modelo fictício privado'
        payload = review.project(result({**FINDING, 'content': f'Usou {model.upper()} na revisão.'}), FILES, (url, model))
        self.assertEqual(payload['items'], [])

    def test_projection_caps_findings_and_comment_length(self):
        files = [{'filename': f'item-{i}.txt', 'patch': '@@ -0,0 +1 @@\n+abc', 'additions': 1, 'deletions': 0}
                 for i in range(22)]
        findings = [{**FINDING, 'path': f['filename'], 'start_line': 1, 'end_line': 1, 'content': 'Exemplo. ' * 150 + 'Fim.'}
                    for f in files]
        payload = review.project(json.dumps({'status': 'complete', 'comments': findings}).encode(), files)
        self.assertEqual(len(payload['items']), 20)
        for comment in review.render(payload, files, HEAD)['comments']:
            self.assertLessEqual(len(comment['body']), 1500)

    def test_budget_only_partial_requires_all_structural_signals(self):
        manifest = dict(schema_version='ocr.run-manifest/v1', operation='review', terminal_state='partial',
                        coverage={'completed': [{'item_id': 'good'}], 'failed': [{'classification': 'budget'}]})
        summary = {'comments': 1, 'budget_exceeded': True}
        warnings = [{'type': 'token_budget_reached'}]
        payload = review.project(result(status='partial', summary=summary, manifest=manifest, warnings=warnings), FILES)
        self.assertTrue(payload['partial'])
        self.assertIn('orçamento', review.render(payload, FILES, HEAD)['body'])
        for tampered in ({**manifest, 'run_failure': {'classification': 'timeout'}},
                         {**manifest, 'coverage': {**manifest['coverage'], 'failed': [{'classification': 'provider'}]}},
                         {**manifest, 'coverage': {**manifest['coverage'], 'completed': []}}):
            with self.assertRaises(review.Refused):
                review.project(result(status='partial', summary=summary, manifest=tampered, warnings=warnings), FILES)

    def test_missing_or_malformed_artifact_never_reads_or_writes_pr(self):
        def request(*_):
            self.fail('unexpected GitHub request')

        with TemporaryDirectory() as tmp:
            artifact = Path(tmp) / 'review.json'
            with self.assertRaises(review.Refused):
                review.publish_artifact(request, 'org/repo', 7, BASE, HEAD, artifact)
            for raw in (b'invalid', b'{}', b'{"items":[],"items":[]}', b' ' * (review.MAX_ARTIFACT + 1)):
                artifact.write_bytes(raw)
                with self.assertRaises(review.Refused):
                    review.publish_artifact(request, 'org/repo', 7, BASE, HEAD, artifact)

    def test_invalid_findings_and_diagnostics_are_refused(self):
        for updates in ({'path': '../outside.txt'}, {'path': 'src/unknown.txt'}, {'start_line': 1},
                        {'end_line': 4}, {'start_line': True}, {'category': 'other @reader'},
                        {'severity': None}):
            with self.subTest(updates=updates), self.assertRaises(review.Refused):
                review.project(result({**FINDING, **updates}), FILES)
        for raw in (b'not json', b'{"status":"failed","status":"complete","comments":[]}',
                    b' ' * (review.MAX_JSON + 1), result(status='partial'), result(warnings=[{'error': 'private'}]),
                    result(summary={'comments': 1, 'budget_exceeded': True}), result(summary={'comments': 0})):
            with self.assertRaises(review.Refused):
                review.project(raw, FILES)

    def test_missing_truncated_or_changed_patch_is_not_publishable(self):
        for patch in (None, '@@ -1,2 +1,3 @@\n same\n+new', '@@ -1,2 +1,3 @@\n same\n+new\n+also new\n-end\n@@ -5 +6 @@\n-old',
                      '@@ -1,2 +1,3 @@\n same\n+new\n+also new\n-end\n@@ -5 +6 @@\n-old\n+new'):
            with self.assertRaises(review.Refused):
                review.project(result(), [{**FILES[0], 'patch': patch}])
        payload = review.project(result(), FILES)
        with self.assertRaises(review.Refused):
            review.render(payload, [{**FILES[0], 'patch': FILES[0]['patch'].replace('new', 'different')}], HEAD)

    def test_handoff_is_revalidated_before_rendering(self):
        payload = review.project(result(), FILES)
        for items in ([[0, 2, 3, 0, 1, '@unsafe']], [[0, 2, 3, 0, 4, 'Texto.']],
                      [[1, 2, 3, 0, 1, 'Texto.']], payload['items'] * 2):
            with self.subTest(items=items), self.assertRaises(review.Refused):
                review.render({**payload, 'items': items}, FILES, HEAD)

    def test_publish_pins_current_head_and_refuses_changed_head(self):
        seen = []
        stale = False

        def request(path, body=None):
            seen.append((path, body))
            if body is not None:
                return None
            if path.endswith('/7'):
                return {'number': 7, 'state': 'open', 'draft': False, 'changed_files': 1,
                        'head': {'sha': 'c' * 40 if stale and len(seen) > 1 else HEAD,
                                 'repo': {'full_name': 'org/repo'}},
                        'base': {'sha': BASE, 'ref': 'main', 'repo': {'full_name': 'org/repo'}}}
            return FILES

        with TemporaryDirectory() as tmp:
            artifact = Path(tmp) / 'review.json'
            artifact.write_text(json.dumps(review.project(result(), FILES)), encoding='utf-8')
            self.assertEqual(review.publish_artifact(request, 'org/repo', 7, BASE, HEAD, artifact), 1)
        self.assertEqual(seen[-1][0], '/repos/org/repo/pulls/7/reviews')
        self.assertEqual(seen[-1][1]['commit_id'], HEAD)
        seen.clear()
        stale = True
        with self.assertRaises(review.Refused):
            review.publish(request, 'org/repo', 7, BASE, HEAD, review.project(result(), FILES))
        self.assertTrue(all(body is None for _, body in seen))

    def test_event_draft_fork_or_moved_base_is_refused_before_reading_files(self):
        pr = {'number': 7, 'state': 'open', 'draft': False, 'changed_files': 1,
              'head': {'sha': HEAD, 'repo': {'full_name': 'org/repo'}},
              'base': {'sha': BASE, 'ref': 'main', 'repo': {'full_name': 'org/repo'}}}
        for changes in ({'draft': True}, {'head': {**pr['head'], 'repo': {'full_name': 'somewhere/repo'}}},
                        {'base': {**pr['base'], 'sha': HEAD}}, {'changed_files': review.MAX_FILES + 1}):
            seen = []

            def request(path, body=None):
                seen.append((path, body))
                return {**pr, **changes}

            with self.subTest(changes=changes), self.assertRaises(review.Refused):
                review.publish(request, 'org/repo', 7, BASE, HEAD, review.project(result(), FILES))
            self.assertEqual(seen, [('/repos/org/repo/pulls/7', None)])


class ProjectCliTest(unittest.TestCase):
    def setUp(self):
        tmp = self.enterContext(TemporaryDirectory())
        work = Path(tmp) / 'mexerica-ocr'
        work.mkdir()
        self.source = work / 'result.json'
        self.artifact = work / 'review.json'
        self.source.write_bytes(result(llm='fixture-model', session_id='fixture-result'))
        self.env = self.enterContext(patch.dict(review.os.environ, {
            'RUNNER_TEMP': tmp, 'PR_NUMBER': '7', 'GITHUB_REPOSITORY': 'fixture/repo',
            'BASE_SHA': BASE, 'HEAD_SHA': HEAD, 'GH_TOKEN': 'fixture-github',
            'OCR_LLM_TOKEN': 'fixture-provider', 'OCR_LLM_URL': 'https://fixture.example.invalid',
            'OCR_LLM_MODEL': 'fixture-model',
        }, clear=True))
        self.enterContext(patch.object(review, 'build_opener'))
        self.snapshot = self.enterContext(patch.object(review, 'snapshot', return_value=('/fixture', FILES)))

    def assert_refused(self, phase=None, args=('project',), *, schema=None):
        stdout, stderr = StringIO(), StringIO()
        with (patch.object(review.sys, 'argv', ['ci-code-review.py', *args]),
              redirect_stdout(stdout), redirect_stderr(stderr), self.assertRaises(SystemExit) as raised):
            review.cli()
        self.assertEqual(raised.exception.code, 1)
        self.assertEqual(stdout.getvalue(), '')
        suffix = '' if phase is None else f' Fase: {phase}.'
        if schema is not None:
            suffix += f' Classe: {schema}.'
        self.assertEqual(stderr.getvalue(), f'Open Code Review: operação recusada ou indisponível.{suffix}\n')
        self.assertFalse(self.artifact.exists())

    def test_snapshot_failures_report_only_snapshot(self):
        with patch.dict(self.env, PR_NUMBER='fixture-pr-number'):
            self.assert_refused('Snapshot')
        self.snapshot.assert_not_called()
        self.snapshot.side_effect = OSError('fixture-snapshot-exception')
        self.assert_refused('Snapshot')

    def test_missing_or_empty_provider_settings_report_only_environment(self):
        for key in ('OCR_LLM_TOKEN', 'OCR_LLM_URL', 'OCR_LLM_MODEL'):
            for missing in (True, False):
                with self.subTest(key=key, missing=missing), patch.dict(self.env):
                    if missing:
                        del self.env[key]
                    else:
                        self.env[key] = ''
                    self.assert_refused('Ambiente')

    def test_missing_file_or_read_exception_reports_only_read(self):
        self.source.unlink()
        self.assert_refused('Leitura')
        with patch.object(Path, 'open') as opened:
            opened.return_value.__enter__.return_value.read.side_effect = OSError('fixture-read-exception')
            self.assert_refused('Leitura')

    def test_projection_failures_report_only_the_current_phase(self):
        cases = (
            ('Json', b'fixture-invalid-json'),
            ('Json', b' ' * (review.MAX_JSON + 1)),
            ('Achado', result({**FINDING, 'path': 'fixture-outside.txt'})),
            ('Achado', result({**FINDING, 'category': 'fixture-category'})),
            ('Render', result({**FINDING, 'start_line': 'fixture-line'})),
            ('Render', result({**FINDING, 'end_line': 4, 'thinking': 'fixture-render'})),
        )
        for phase, data in cases:
            with self.subTest(phase=phase):
                self.source.write_bytes(data)
                self.assert_refused(phase)

    def test_schema_reports_only_the_first_fixed_class(self):
        manifest = dict(schema_version='ocr.run-manifest/v1', operation='review', terminal_state='complete',
                        coverage={'failed': []})
        budget = {'comments': 1, 'budget_exceeded': True}
        cases = (
            ('Estrutura', b'["fixture-structure"]'),
            ('Estrutura', result(fixture_field='fixture-schema', status='fixture-status')),
            ('Comentarios', result(comments='fixture-comments', status='fixture-status')),
            ('Estado', result(status='fixture-status', summary='fixture-summary')),
            ('Resumo', result(summary={'comments': False}, warnings='fixture-warnings')),
            ('Avisos', result(warnings='fixture-warnings', manifest='fixture-manifest')),
            ('Manifesto', result(manifest={**manifest, 'terminal_state': 'fixture-state'}, warnings=[{}])),
            ('Avisos', result(manifest=manifest, summary=budget, warnings=[{'type': 'token_budget_reached'}])),
            ('Resumo', result(manifest=manifest, summary=budget)),
            ('Estado', result(status='skipped')),
            ('Cobertura', result(manifest={**manifest, 'coverage': {'failed': [{}]}})),
            ('Resumo', result(status='partial')),
            ('Manifesto', result(status='partial', summary=budget)),
            ('Avisos', result(status='partial', summary=budget, manifest={**manifest, 'terminal_state': 'partial'},
                              warnings=[{'type': 'review_limit_reached', 'file': 'fixture-group',
                                         'message': 'fixture-warning'}])),
            ('Cobertura', result(status='partial', summary=budget, manifest={**manifest, 'terminal_state': 'partial',
                                                                            'coverage': 'fixture-coverage'})),
        )
        for index, (schema, data) in enumerate(cases):
            with self.subTest(case=index, schema=schema):
                self.source.write_bytes(data)
                self.assert_refused('Esquema', schema=schema)

    def test_complete_upstream_result_with_warning_stays_refused(self):
        item = {'item_id': 'fixture-item', 'path': FILES[0]['filename']}
        self.source.write_bytes(result(
            summary={'files_reviewed': 1, 'comments': 1, 'input_tokens': 10, 'output_tokens': 2,
                     'total_tokens': 12, 'elapsed': '1s'},
            tool_calls={'total': 0, 'by_tool': {}, 'failure': 0, 'failure_by_tool': {}, 'failure_details': []},
            manifest={'schema_version': 'ocr.run-manifest/v1', 'run_id': 'fixture-run', 'operation': 'review',
                      'terminal_state': 'complete', 'repository': {}, 'input': {'mode': 'range'}, 'execution': {},
                      'coverage': {'selected': [item], 'completed': [item], 'reused': [], 'failed': [],
                                   'waived': [], 'excluded': []}, 'elapsed_ms': 1000},
            warnings=[{'type': 'review_round_failed', 'file': 'fixture-group', 'message': 'fixture-warning'}],
        ))
        self.assert_refused('Esquema', schema='Avisos')

    def test_write_limit_and_partial_write_report_only_write(self):
        with patch.object(review, 'MAX_ARTIFACT', 1):
            self.assert_refused('Gravacao')

        write_bytes = Path.write_bytes

        def partial_write(path, data):
            self.assertEqual(path, self.artifact)
            write_bytes(path, data[:8])
            raise OSError('fixture-write-exception')

        with patch.object(Path, 'write_bytes', partial_write):
            self.assert_refused('Gravacao')
        self.assertTrue(self.source.is_file())

    def test_failure_without_project_phase_keeps_the_original_message(self):
        self.assert_refused(args=())
        self.snapshot.side_effect = OSError('fixture-snapshot-exception')
        self.assert_refused(args=('preflight',))

    def test_valid_project_is_silent_and_writes_the_same_payload(self):
        expected = review.project(result(), FILES)
        stdout, stderr = StringIO(), StringIO()
        with (patch.object(review.sys, 'argv', ['ci-code-review.py', 'project']),
              redirect_stdout(stdout), redirect_stderr(stderr)):
            review.cli()
        self.assertEqual(stdout.getvalue(), '')
        self.assertEqual(stderr.getvalue(), '')
        self.assertEqual(self.artifact.read_bytes(),
                         json.dumps(expected, ensure_ascii=False, separators=(',', ':')).encode('utf-8'))


if __name__ == '__main__':
    unittest.main()
