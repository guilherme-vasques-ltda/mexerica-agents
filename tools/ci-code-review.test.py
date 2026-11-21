import importlib.util
import json
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location('review', Path(__file__).with_name('ci-code-review.py'))
review = importlib.util.module_from_spec(spec)
spec.loader.exec_module(review)

HEAD = 'a' * 40
BASE = 'b' * 40
FILES = [{'filename': 'src/synthetic.txt', 'patch': '@@ -1,2 +1,3 @@\n same\n+new\n+also new\n-end',
          'additions': 2, 'deletions': 1}]
FINDING = {'path': 'src/synthetic.txt', 'start_line': 2, 'end_line': 3,
           'category': 'bug', 'severity': 'high', 'content': 'PRIVATE-MARKER @reader https://review.example.invalid'}


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
        for items in ([[0, 2, 3, 0, 'RAW']], [[0, 2, 3, 0, 4]], [[1, 2, 3, 0, 1]], payload['items'] * 2):
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

        self.assertEqual(review.publish(request, 'org/repo', 7, BASE, HEAD, review.project(result(), FILES)), 1)
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


if __name__ == '__main__':
    unittest.main()
