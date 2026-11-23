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
LIMIT_WARNING = {'type': 'review_limit_reached', 'file': 'fixture-warning-file', 'message': 'fixture-warning-message'}
ROUND_WARNING = {'type': 'review_round_failed', 'file': 'fixture-group-name', 'message': 'fixture-raw-error'}


def result(finding=FINDING, **overrides):
    return json.dumps({'status': 'complete', 'comments': [finding], **overrides}).encode()


def limit_partial(**overrides):
    manifest = dict(schema_version='ocr.run-manifest/v1', operation='review', terminal_state='partial',
                    coverage={'completed': [{'item_id': 'fixture-completed'}], 'failed': [{'classification': 'budget'}]})
    return result(**{'status': 'partial', 'summary': {'comments': 0, 'budget_exceeded': True}, 'comments': [],
                     'manifest': manifest, 'warnings': [LIMIT_WARNING], **overrides})


def limit_complete(**overrides):
    items = [{'item_id': key * 64, 'path': f'{key}.txt'} for key in ('a', 'b')]
    return result(**{'comments': [], 'summary': {'comments': 0, 'files_reviewed': 2},
                     'manifest': dict(schema_version='ocr.run-manifest/v1', operation='review', terminal_state='complete',
                                      coverage=dict(selected=items, completed=items[::-1], failed=[], waived=[], reused=[])),
                     'warnings': [LIMIT_WARNING, {**LIMIT_WARNING, 'file': '', 'message': ''}], **overrides})


def round_complete(**overrides):
    return limit_complete(**{'warnings': [ROUND_WARNING], **overrides})


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
        self.assertEqual(payload, {'diff': review.indexed(FILES)[1], 'items': [[0, 2, 3, 0, 1, FINDING['content']]],
                                   'partial': True})
        self.assertEqual(review.project(result(status='partial', summary=summary, manifest=manifest, warnings=[]),
                                        FILES), payload)
        self.assertIn('limite de revisão', review.render(payload, FILES, HEAD)['body'])
        for tampered in ({**manifest, 'run_failure': {'classification': 'timeout'}},
                         {**manifest, 'coverage': {**manifest['coverage'], 'failed': [{'classification': 'provider'}]}},
                         {**manifest, 'coverage': {**manifest['coverage'], 'completed': []}}):
            with self.assertRaises(review.Refused):
                review.project(result(status='partial', summary=summary, manifest=tampered, warnings=warnings), FILES)

    def test_review_limit_partial_projects_no_warning_data_and_publishes_comment_without_findings(self):
        payload = review.project(limit_partial(), FILES)
        self.assertEqual(payload, {'diff': review.indexed(FILES)[1], 'items': [], 'partial': True})
        for warnings in ([{'type': 'token_budget_reached'}, LIMIT_WARNING],
                         [{**LIMIT_WARNING, 'file': '', 'message': ''}]):
            self.assertEqual(review.project(limit_partial(warnings=warnings), FILES), payload)
        with (TemporaryDirectory() as tmp,
              patch.object(review, 'snapshot', return_value=('/fixture', FILES))):
            artifact = Path(tmp) / 'review.json'
            artifact.write_text(json.dumps(payload), encoding='utf-8')
            published = []
            count = review.publish_artifact(lambda path, body: published.append((path, body)),
                                            'fixture/repo', 7, BASE, HEAD, artifact)
        self.assertEqual(count, 0)
        self.assertEqual(published, [('/fixture/reviews', {
            'commit_id': HEAD, 'event': 'COMMENT', 'comments': [],
            'body': 'Open Code Review: 0 sinalização(ões) para verificação manual. Não é uma aprovação.'
                    ' Revisão parcial: limite de revisão atingido.',
        })])

    def test_complete_limit_projects_only_validated_findings_and_a_partial_flag(self):
        clean = review.project(result(), FILES)
        summary = {'comments': 1, 'files_reviewed': 2}
        for warnings in ([LIMIT_WARNING], [LIMIT_WARNING] * 20):
            for budget in ({}, {'budget_exceeded': False}):
                with self.subTest(warnings=len(warnings), budget=budget):
                    self.assertEqual(review.project(limit_complete(comments=[FINDING], summary={**summary, **budget},
                                                                   warnings=warnings), FILES), {**clean, 'partial': True})
        unsafe = {**FINDING, 'content': LIMIT_WARNING['message'], 'thinking': 'fixture-private-finding'}
        self.assertEqual(review.project(limit_complete(comments=[unsafe], summary=summary), FILES,
                                        ('fixture-private-finding',)), {**clean, 'items': [], 'partial': True})
        with self.assertRaises(review.Refused):
            review.project(limit_complete(comments=[{**FINDING, 'end_line': 4}], summary=summary), FILES)

    def test_complete_limit_publishes_fixed_comment_even_without_findings(self):
        limited = review.project(limit_complete(), FILES)
        no_warning = json.loads(limit_complete())
        del no_warning['warnings']
        clean = review.project(json.dumps(no_warning).encode(), FILES)
        clean_with_empty_warnings = review.project(limit_complete(warnings=[]), FILES)
        skipped = review.project(result(status='skipped', comments=[]), FILES)
        self.assertEqual(limited, {'diff': review.indexed(FILES)[1], 'items': [], 'partial': True})
        self.assertEqual(clean, {**limited, 'partial': False})
        self.assertEqual(clean_with_empty_warnings, clean)
        self.assertEqual(skipped, clean)
        with (TemporaryDirectory() as tmp,
              patch.object(review, 'snapshot', return_value=('/fixture', FILES))):
            artifact = Path(tmp) / 'review.json'
            published = []
            for payload in (clean, skipped, limited):
                artifact.write_text(json.dumps(payload), encoding='utf-8')
                self.assertEqual(review.publish_artifact(lambda path, body: published.append((path, body)),
                                                         'fixture/repo', 7, BASE, HEAD, artifact), 0)
        self.assertEqual(published, [('/fixture/reviews', {
            'commit_id': HEAD, 'event': 'COMMENT', 'comments': [],
            'body': 'Open Code Review: 0 sinalização(ões) para verificação manual. Não é uma aprovação.'
                    ' Revisão parcial: limite de revisão atingido.',
        })])

    def test_complete_round_failure_adds_only_a_fixed_reason_and_keeps_finding_filters(self):
        summary = {'comments': 2}
        unsafe = {**FINDING, 'content': 'Texto filtrado.', 'thinking': 'fixture-private-finding'}
        clean = review.project(result(), FILES)
        for warnings in ([ROUND_WARNING], [ROUND_WARNING] * 20):
            for budget in ({}, {'budget_exceeded': False}):
                with self.subTest(count=len(warnings), budget=budget):
                    raw = round_complete(comments=[FINDING, unsafe], summary={**summary, **budget}, warnings=warnings)
                    self.assertEqual(review.project(raw, FILES, ('fixture-private-finding',)),
                                     {**clean, 'partial': True, 'partial_reason': 'round_failed'})
        self.assertEqual(review.project(round_complete(warnings=[]), FILES),
                         {'diff': clean['diff'], 'items': [], 'partial': False})
        with self.assertRaises(review.Refused):
            review.project(round_complete(comments=[{**FINDING, 'end_line': 4}], summary={'comments': 1}), FILES)

    def test_complete_round_failure_publishes_one_fixed_comment(self):
        for comments in ([], [FINDING]):
            with self.subTest(count=len(comments)), TemporaryDirectory() as tmp:
                payload = review.project(round_complete(comments=comments, summary={'comments': len(comments)}), FILES)
                clean = review.project(result(comments=comments), FILES)
                self.assertEqual(payload, {**clean, 'partial': True, 'partial_reason': 'round_failed'})
                artifact = Path(tmp) / 'review.json'
                artifact.write_text(json.dumps(payload), encoding='utf-8')
                published = []
                with patch.object(review, 'snapshot', return_value=('/fixture', FILES)):
                    self.assertEqual(review.publish_artifact(lambda path, body: published.append((path, body)),
                                                             'fixture/repo', 7, BASE, HEAD, artifact), len(comments))
                expected = review.render(clean, FILES, HEAD)
                expected['body'] = (f'Open Code Review: {len(comments)} sinalização(ões) para verificação manual. '
                                    'Não é uma aprovação. Revisão parcial: rodada adicional falhou.')
                self.assertEqual(published, [('/fixture/reviews', expected)])
                self.assertEqual(published[0][1]['event'], 'COMMENT')

    def test_handoff_rejects_noncanonical_partial_reasons_before_github_requests(self):
        clean = review.project(result(), FILES)
        invalid = [{**clean, 'partial_reason': 'round_failed'},
                   {**clean, 'partial': 1, 'partial_reason': 'round_failed'},
                   {**clean, 'partial': True, 'partial_reason': 'round_failed', 'extra': ''}]
        invalid.extend({**clean, 'partial': True, 'partial_reason': reason}
                       for reason in (None, True, 1, [], {}, '', 'ROUND_FAILED', 'round_failed ', 'review_limit_reached'))
        with TemporaryDirectory() as tmp:
            artifact = Path(tmp) / 'review.json'
            for payload in invalid:
                with self.subTest(payload=payload), self.assertRaises(review.Refused):
                    artifact.write_text(json.dumps(payload), encoding='utf-8')
                    review.publish_artifact(lambda *_: self.fail('unexpected GitHub request'),
                                            'fixture/repo', 7, BASE, HEAD, artifact)

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

    def assert_refused(self, phase=None, args=('project',), *, schema=None, warning_type=None):
        stdout, stderr = StringIO(), StringIO()
        with (patch.object(review.sys, 'argv', ['ci-code-review.py', *args]),
              redirect_stdout(stdout), redirect_stderr(stderr), self.assertRaises(SystemExit) as raised):
            review.cli()
        self.assertEqual(raised.exception.code, 1)
        self.assertEqual(stdout.getvalue(), '')
        suffix = '' if phase is None else f' Fase: {phase}.'
        if schema is not None:
            suffix += f' Classe: {schema}.'
        if warning_type is not None:
            suffix += f' Tipo: {warning_type}.'
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
            ('AvisosFormato', result(warnings='fixture-warnings', manifest='fixture-manifest')),
            ('Manifesto', result(manifest={**manifest, 'terminal_state': 'fixture-state'}, warnings=[{}])),
            ('Resumo', result(manifest=manifest, summary=budget)),
            ('Estado', result(status='skipped')),
            ('Cobertura', result(manifest={**manifest, 'coverage': {'failed': [{}]}})),
            ('Resumo', result(status='partial')),
            ('Manifesto', result(status='partial', summary=budget)),
            ('Cobertura', result(status='partial', summary=budget, manifest={**manifest, 'terminal_state': 'partial',
                                                                            'coverage': 'fixture-coverage'})),
        )
        for index, (schema, data) in enumerate(cases):
            with self.subTest(case=index, schema=schema):
                self.source.write_bytes(data)
                self.assert_refused('Esquema', schema=schema)

    def test_warning_types_are_fixed_and_complete_reports_only_the_first_warning(self):
        for input_type, category in (
            ('manifest_error', 'ErroManifesto'),
            ('token_budget_reached', 'OrcamentoTokens'),
            ('subtask_error', 'ErroSubtarefa'),
            ('token_threshold_exceeded', 'LimiarTokens'),
            ('review_round_failed', 'FalhaRodada'),
            ('review_limit_reached', 'LimiteRevisao'),
            ('comment_args_repaired', 'ArgumentosComentarioReparados'),
            ('comment_refiled', 'ComentarioRealocado'),
            ('fixture-unknown-type', 'Outro'),
        ):
            with self.subTest(category=category):
                self.source.write_bytes(result(summary={'comments': 1, 'budget_exceeded': True}, warnings=[
                    {'type': input_type, 'file': 'fixture-warning-file', 'message': 'fixture-warning-message'},
                    {'type': 'review_round_failed'},
                ]))
                self.assert_refused('Esquema', schema='AvisosEmCompleto', warning_type=category)

    def test_skipped_reports_the_first_warning_before_refusing_findings(self):
        self.source.write_bytes(result(status='skipped', warnings=[
            'fixture-non-object-warning', {'type': 'review_round_failed'},
        ]))
        self.assert_refused('Esquema', schema='AvisosEmIgnorado', warning_type='Outro')

    def test_partial_reports_the_first_refused_warning_before_coverage(self):
        for first_refused, category in (
            ({'type': 'review_limit_reached', 'file': 'fixture-warning-file'}, 'LimiteRevisao'),
            ({'type': 'fixture-unknown-type'}, 'Outro'),
            ({'type': ['fixture-non-string-type']}, 'Outro'),
            ({}, 'Outro'),
            ('fixture-non-object-warning', 'Outro'),
        ):
            with self.subTest(category=category):
                self.source.write_bytes(result(
                    status='partial', summary={'comments': 1, 'budget_exceeded': True},
                    manifest=dict(schema_version='ocr.run-manifest/v1', operation='review', terminal_state='partial',
                                  coverage='fixture-coverage'),
                    warnings=[{'type': 'token_budget_reached'}, LIMIT_WARNING, first_refused, {'type': 'review_round_failed'}],
                ))
                self.assert_refused('Esquema', schema='AvisosItem', warning_type=category)

    def test_review_limit_warning_requires_exact_fields_and_types(self):
        cases = [({**LIMIT_WARNING, 'extra': 'fixture-extra'}, 'LimiteRevisao')]
        for key in LIMIT_WARNING:
            category = 'Outro' if key == 'type' else 'LimiteRevisao'
            cases.extend((({k: v for k, v in LIMIT_WARNING.items() if k != key}, category),
                          ({**LIMIT_WARNING, key: 1}, category)))
        for warning, category in cases:
            with self.subTest(warning=warning):
                self.source.write_bytes(limit_partial(warnings=[warning]))
                self.assert_refused('Esquema', schema='AvisosItem', warning_type=category)

    def test_review_limit_does_not_bypass_state_budget_manifest_or_coverage(self):
        manifest = json.loads(limit_partial())['manifest']
        cases = [
            (result(comments=[], warnings=[LIMIT_WARNING]), 'Manifesto', None),
            (result(status='skipped', comments=[], warnings=[LIMIT_WARNING]), 'AvisosEmIgnorado', 'LimiteRevisao'),
            (limit_partial(summary={'comments': 0, 'budget_exceeded': 1}), 'Resumo', None),
            (limit_partial(manifest=None), 'Manifesto', None),
            (limit_partial(manifest={**manifest, 'terminal_state': 'complete'}), 'Manifesto', None),
            (limit_partial(manifest={**manifest, 'run_failure': {'classification': 'timeout'}}), 'Manifesto', None),
            (limit_partial(manifest={**manifest, 'coverage': {**manifest['coverage'], 'completed': []}}), 'Cobertura', None),
            (limit_partial(warnings=[LIMIT_WARNING, {'type': 'review_round_failed'}]), 'AvisosItem', 'FalhaRodada'),
            (limit_partial(extra='fixture-extra'), 'Estrutura', None),
        ]
        for classification in ('provider', 'timeout'):
            coverage = {**manifest['coverage'], 'failed': manifest['coverage']['failed'] + [{'classification': classification}]}
            cases.append((limit_partial(manifest={**manifest, 'coverage': coverage}), 'Cobertura', None))
        for index, (data, schema, category) in enumerate(cases):
            with self.subTest(case=index, schema=schema):
                self.source.write_bytes(data)
                self.assert_refused('Esquema', schema=schema, warning_type=category)

    def test_complete_upstream_result_with_warning_and_invalid_coverage_stays_refused(self):
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
        self.assert_refused('Esquema', schema='Cobertura')

    def test_complete_limit_refuses_invalid_warning_even_when_surrounded_by_valid_items(self):
        malformed = [None, LIMIT_WARNING['type'], {**LIMIT_WARNING, 'extra': 'fixture-extra'}]
        for key in LIMIT_WARNING:
            malformed.extend(({k: v for k, v in LIMIT_WARNING.items() if k != key}, {**LIMIT_WARNING, key: 1}))
        for kind in ('token_budget_reached', 'manifest_error', 'subtask_error', 'review_round_failed',
                     'review_limit_reached ', 'REVIEW_LIMIT_REACHED'):
            malformed.append({**LIMIT_WARNING, 'type': kind})
        for item in malformed:
            with self.subTest(item=item):
                self.source.write_bytes(limit_complete(warnings=[LIMIT_WARNING, item, LIMIT_WARNING]))
                self.assert_refused('Esquema', schema='AvisosEmCompleto', warning_type='LimiteRevisao')
        self.source.write_bytes(limit_complete(warnings=[LIMIT_WARNING] * 21))
        self.assert_refused('Esquema', schema='AvisosEmCompleto', warning_type='LimiteRevisao')
        self.source.write_bytes(limit_complete(warnings=LIMIT_WARNING))
        self.assert_refused('Esquema', schema='AvisosFormato')

    def test_complete_round_failure_refuses_mixed_or_malformed_warnings(self):
        malformed = [None, 'review_round_failed', {**ROUND_WARNING, 'extra': ''}]
        for key in ROUND_WARNING:
            malformed.extend(({k: v for k, v in ROUND_WARNING.items() if k != key}, {**ROUND_WARNING, key: 1}))
        for kind in ('token_budget_reached', 'manifest_error', 'subtask_error', 'review_round_failed ', 'REVIEW_ROUND_FAILED'):
            malformed.append({**ROUND_WARNING, 'type': kind})
        for item in malformed:
            with self.subTest(item=item), self.assertRaises(review.Refused):
                review.project(round_complete(warnings=[ROUND_WARNING, item]), FILES)
        for warnings in ([ROUND_WARNING, LIMIT_WARNING], [LIMIT_WARNING, ROUND_WARNING], [ROUND_WARNING] * 21,
                         ROUND_WARNING, None):
            with self.subTest(warnings=warnings), self.assertRaises(review.Refused):
                review.project(round_complete(warnings=warnings), FILES)

    def test_first_round_error_or_fatal_failure_stays_refused(self):
        manifest = json.loads(round_complete())['manifest']
        coverage = manifest['coverage']
        self.source.write_bytes(round_complete(warnings=[{**ROUND_WARNING, 'type': 'subtask_error'}],
                                               manifest={**manifest, 'coverage': {**coverage, 'completed': [],
                                                         'failed': coverage['selected']}}))
        self.assert_refused('Esquema', schema='AvisosEmCompleto', warning_type='ErroSubtarefa')
        self.source.write_bytes(round_complete(status='error', manifest={**manifest, 'terminal_state': 'error'}))
        self.assert_refused('Esquema', schema='Estado')
        self.source.write_bytes(round_complete(manifest={**manifest, 'run_failure': {'classification': 'timeout'}}))
        self.assert_refused('Esquema', schema='Manifesto')
        self.source.write_bytes(round_complete(status='skipped', manifest={**manifest, 'terminal_state': 'skipped'}))
        self.assert_refused('Esquema', schema='AvisosEmIgnorado', warning_type='FalhaRodada')
        self.source.write_bytes(limit_partial(warnings=[ROUND_WARNING]))
        self.assert_refused('Esquema', schema='AvisosItem', warning_type='FalhaRodada')

    def test_complete_warning_requires_consistent_manifest_and_summary(self):
        data = json.loads(limit_complete())
        manifest = data['manifest']
        cases = [('Manifesto', {'manifest': None}), ('Manifesto', {'manifest': 'fixture-manifest'}),
                 ('Manifesto', {'resume': {}}), ('Resumo', {'summary': None}), ('Resumo', {'summary': []}),
                 ('Resumo', {'summary': {'files_reviewed': 2}}), ('Resumo', {'summary': {'comments': False}}),
                 ('Resumo', {'summary': {'comments': 1}})]
        for key, value in (('terminal_state', 'partial'), ('schema_version', 'fixture-schema'), ('operation', 'scan'),
                           ('run_failure', {'classification': 'timeout'}), ('run_failure', None),
                           ('parent_run_id', 'fixture-parent')):
            cases.append(('Manifesto', {'manifest': {**manifest, key: value}}))
        for budget in (True, 0, None, 'false'):
            cases.append(('Resumo', {'summary': {'comments': 0, 'budget_exceeded': budget}}))
        for fixture in (limit_complete, round_complete):
            for schema, overrides in cases:
                with self.subTest(fixture=fixture.__name__, overrides=overrides):
                    self.source.write_bytes(fixture(**overrides))
                    self.assert_refused('Esquema', schema=schema)
            data = json.loads(fixture())
            for key, schema in (('manifest', 'Manifesto'), ('summary', 'Resumo')):
                with self.subTest(fixture=fixture.__name__, missing=key):
                    self.source.write_text(json.dumps({k: v for k, v in data.items() if k != key}), encoding='utf-8')
                    self.assert_refused('Esquema', schema=schema)

    def test_complete_warning_requires_matching_selected_and_completed_without_other_outcomes(self):
        manifest = json.loads(limit_complete())['manifest']
        coverage = manifest['coverage']
        cases = [None, [], {**coverage, 'completed': [{'item_id': 'c' * 64}]}]
        for key in ('selected', 'completed', 'failed', 'waived', 'reused'):
            cases.append({k: v for k, v in coverage.items() if k != key})
            cases.append({**coverage, key: None})
            if key in ('selected', 'completed'):
                for values in ([], {}, [None], [{}], [{'item_id': ''}], [{'item_id': 42}],
                               [{'item_id': 'fixture-not-a-digest'}], coverage[key] + coverage[key][:1],
                               [{**coverage[key][0], 'classification': 'budget'}],
                               [{**coverage[key][0], 'reason': 'fixture-reason'}]):
                    cases.append({**coverage, key: values})
            else:
                cases.extend(({**coverage, key: {}}, {**coverage, key: coverage['selected'][:1]}))
        for fixture in (limit_complete, round_complete):
            for candidate in cases:
                with self.subTest(fixture=fixture.__name__, coverage=candidate):
                    self.source.write_bytes(fixture(manifest={**manifest, 'coverage': candidate}))
                    self.assert_refused('Esquema', schema='Cobertura')

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
        for raw in (result(), limit_partial()):
            self.source.write_bytes(raw)
            expected = review.project(raw, FILES)
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
