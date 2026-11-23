"""Testa a classificação e a expiração do passo Review diff sem rodar o revisor.

Rótulos: Prazo = status 124, compatível com prazo do GNU timeout (não prova inequívoca de prazo).
Sinal = status convencional 128+1..64 (137 e 143 podem vir de kill-after ou de outro sinal; a origem não é confirmada).
Saida = demais status (1, 125, 126, 127 etc.); a origem não é afirmada.
"""
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = (ROOT / '.github/workflows/code-review.yml').read_text(encoding='utf-8')
RUN = WORKFLOW.split('      - name: Review diff\n', 1)[1].split('      - name: Validate review output\n', 1)[0].split('        run: |\n', 1)[1]
BLOCK = '          { if (\n' + RUN.split('          { if (\n', 1)[1].split('\n          } 2>/dev/null\n', 1)[0] + '\n          } 2>/dev/null\n'
TIMEOUT_CMD = 'exec timeout --kill-after=5s 480s /usr/bin/bwrap'
PRIVATE = ('PRIVATE_STDOUT', 'PRIVATE_STDERR')
CASES = ((0, None), (1, 'Saida'), (124, 'Prazo'), (125, 'Saida'), (126, 'Saida'), (127, 'Saida'),
         (137, 'Sinal'), (143, 'Sinal'), (('signal', 15), 'Sinal'), (('signal', 9), 'Sinal'))


def fake_timeout(root, behavior):
    bin_dir = root / 'bin'
    bin_dir.mkdir()
    fake = bin_dir / 'timeout'
    if isinstance(behavior, tuple):
        end = f'kill -{behavior[1]} $$'
    else:
        end = f'exit {behavior}'
    fake.write_text(f'#!/bin/sh\nprintf "%s" "{PRIVATE[0]}"\nprintf "%s" "{PRIVATE[1]}" >&2\n{end}\n')
    fake.chmod(0o755)
    return bin_dir


def run_step(root, bin_dir, summary, block=BLOCK):
    work = root / 'work'
    (work / 'profile').mkdir(parents=True)
    (work / 'tmp').mkdir()
    (work / 'merge-base').write_text('b' * 40 + '\n')
    env = {'PATH': f'{bin_dir}:/usr/bin:/bin' if bin_dir else '/usr/bin:/bin',
           'HEAD_SHA': 'a' * 40, 'OCR_EFFORT': 'medium',
           'OCR_MAX_TOKENS_BUDGET': '1', 'OCR_LLM_URL': 'x', 'OCR_LLM_TOKEN': 'x',
           'OCR_LLM_MODEL': 'x', 'OCR_LLM_PROTOCOL': 'x'}
    if summary is not None:
        env['GITHUB_STEP_SUMMARY'] = str(summary)
    script = 'work=$1\nprivate_home="$work/profile"\n' + block
    proc = subprocess.run(['/bin/bash', '--noprofile', '--norc', '-c', script, 'review-step', str(work)],
                          env=env, stdin=subprocess.DEVNULL, capture_output=True, text=True,
                          start_new_session=True, timeout=6)
    return proc, work


class StatusTest(unittest.TestCase):
    def test_status_is_categorized_without_leaking_child_streams(self):
        for behavior, category in CASES:
            with self.subTest(status=behavior), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                summary = root / 'summary'
                proc, work = run_step(root, fake_timeout(root, behavior), summary)
                self.assertNotIn('PRIVATE_', proc.stdout + proc.stderr)
                self.assertEqual(proc.stderr, '')
                self.assertIn('PRIVATE_STDOUT', (work / 'result.json').read_text())
                self.assertIn('PRIVATE_STDERR', (work / 'stderr.log').read_text())
                if category is None:
                    self.assertEqual((proc.returncode, proc.stdout), (0, ''))
                    self.assertFalse(summary.exists())
                    continue
                self.assertEqual(proc.returncode, 1)
                self.assertEqual(proc.stdout, f'Open Code Review: {category}.\nOpen Code Review: review unavailable.\n')
                self.assertEqual(summary.read_text(), f'Open Code Review: {category}.\n')

    def test_unusable_summary_is_silent_and_keeps_exit_one(self):
        for mode in ('unset', 'empty', 'directory', 'missing-parent'):
            with self.subTest(summary=mode), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                bad = root / 'bad'
                if mode == 'directory':
                    bad.mkdir()
                value = {'unset': None, 'empty': '', 'directory': bad, 'missing-parent': root / 'absent' / 'summary'}[mode]
                proc, _ = run_step(root, fake_timeout(root, 124), value)
                self.assertEqual(proc.returncode, 1)
                self.assertEqual(proc.stdout, 'Open Code Review: Prazo.\nOpen Code Review: review unavailable.\n')
                self.assertEqual(proc.stderr, '')
                self.assertNotIn(str(root), proc.stdout + proc.stderr)
                self.assertNotIn('PRIVATE_', proc.stdout + proc.stderr)

    @unittest.skipUnless(sys.platform == 'linux', 'requer GNU timeout e ps no Linux')
    def test_real_deadline_stops_mock_process_group(self):
        version = subprocess.check_output(['timeout', '--version'], text=True,
                                          env={'PATH': '/usr/bin:/bin'}, timeout=2)
        self.assertIn('GNU coreutils', version)
        self.assertEqual(BLOCK.count(TIMEOUT_CMD), 1)
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            summary = root / 'summary'
            mock = root / 'mock-reviewer'
            mock.write_text('#!/bin/bash --norc\nsleep 4 &\nchild=$!\n'
                            'trap \'wait "$child"; exit 0\' TERM\n'
                            'ps -o pgid= -p "$$,$child" > "${0%/*}/groups"\nwait "$child"\n')
            mock.chmod(0o755)
            block = BLOCK.replace(TIMEOUT_CMD, 'exec timeout --kill-after=0.2s 1s "${work%/*}/mock-reviewer"')
            proc, _ = run_step(root, None, summary, block)
            self.assertEqual((proc.returncode, proc.stdout, proc.stderr), (1,
                             'Open Code Review: Prazo.\nOpen Code Review: review unavailable.\n', ''))
            self.assertEqual(summary.read_text(), 'Open Code Review: Prazo.\n')
            groups = (root / 'groups').read_text().split()
            self.assertEqual(len(groups), 2)
            self.assertEqual(groups[0], groups[1])
            processes = subprocess.check_output(['ps', '-e', '-o', 'pgid=,stat='], text=True, timeout=2)
            active = [state for group, state in map(str.split, processes.splitlines())
                      if group == groups[0] and not state.startswith(('Z', 'X'))]
            self.assertEqual(active, [])


if __name__ == '__main__':
    unittest.main()
