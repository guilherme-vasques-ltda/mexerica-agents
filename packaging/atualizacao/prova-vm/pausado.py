#!/usr/bin/env python3
"""Deixa uma chamada do wrapper parada no exec (ptrace), como uma chamada já iniciada e
suspensa antes de obter a trava. Uso: pausado.py WRAPPER ARQUIVO_PID. Imprime como terminou."""
import ctypes, os, sys
libc = ctypes.CDLL(None, use_errno=True)
pid = os.fork()
if pid == 0:
    libc.ptrace(0, 0, None, None)  # PTRACE_TRACEME: para no exec
    os.execv(sys.argv[1], [sys.argv[1], "--aceitar"])
os.waitpid(pid, os.WUNTRACED)
with open(sys.argv[2], "w") as f:
    f.write(str(pid))
_, st = os.waitpid(pid, 0)
print("sinal", os.WTERMSIG(st) if os.WIFSIGNALED(st) else None, "status", os.WEXITSTATUS(st) if os.WIFEXITED(st) else None)
