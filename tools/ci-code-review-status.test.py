"""Testa a classificação de status do passo Review diff com subprocesso falso.

Rótulos: Prazo = status 124, compatível com prazo do GNU timeout (não prova inequívoca de prazo).
Sinal = status convencional 128+1..64 (137 e 143 podem vir de kill-after ou de outro sinal; a origem não é confirmada).
Saida = demais status (1, 125, 126, 127 etc.); a origem não é afirmada.
"""
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = (ROOT / '.github/workflows/code-review.yml').read_text(encoding='utf-8')
RUN = WORKFLOW.split('      - name: Review diff\n', 1)[1].split('      - name: Validate review output\n', 1)[0].split('        run: |\n', 1)[1]
BLOCK = '          { if (\n' + RUN.split('          { if (\n', 1)[1].split('\n          } 2>/dev/null\n', 1)[0] + '\n          } 2>/dev/null\n'
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


def run_step(root, behavior, summary):
    bin_dir = fake_timeout(root, behavior)
    work = root / 'work'
    (work / 'profile').mkdir(parents=True)
    (work / 'tmp').mkdir()
    (work / 'merge-base').write_text('b' * 40 + '\n')
    env = {'PATH': f'{bin_dir}:/usr/bin:/bin', 'HEAD_SHA': 'a' * 40, 'OCR_EFFORT': 'medium',
           'OCR_MAX_TOKENS_BUDGET': '1', 'OCR_LLM_URL': 'x', 'OCR_LLM_TOKEN': 'x',
           'OCR_LLM_MODEL': 'x', 'OCR_LLM_PROTOCOL': 'x'}
    if summary is not None:
        env['GITHUB_STEP_SUMMARY'] = str(summary)
    script = f'work={work}\nprivate_home="$work/profile"\n' + BLOCK
    proc = subprocess.run(['/bin/bash', '-c', script], env=env, capture_output=True, text=True)
    return proc, work


class StatusTest(unittest.TestCase):
    def test_status_is_categorized_without_leaking_child_streams(self):
        for behavior, category in CASES:
            with self.subTest(status=behavior), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                summary = root / 'summary'
                proc, work = run_step(root, behavior, summary)
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
                proc, _ = run_step(root, 124, value)
                self.assertEqual(proc.returncode, 1)
                self.assertEqual(proc.stdout, 'Open Code Review: Prazo.\nOpen Code Review: review unavailable.\n')
                self.assertEqual(proc.stderr, '')
                self.assertNotIn(str(root), proc.stdout + proc.stderr)
                self.assertNotIn('PRIVATE_', proc.stdout + proc.stderr)


if __name__ == '__main__':
    unittest.main()
