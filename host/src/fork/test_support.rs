//! Executáveis fictícios: o processo multithread nunca abre o destino para escrita.
#![allow(clippy::unwrap_used)]
use std::{
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
};

pub(crate) fn write_executable(path: &Path, contents: &[u8], mode: u32) {
    write_executable_during(path, contents, mode, || {});
}

fn write_executable_during(path: &Path, contents: &[u8], mode: u32, during_write: impl FnOnce()) {
    // Somente o filho abre o script. Um fork em qualquer outra thread pode herdar
    // os pipes, mas nunca o fd de escrita do executável. Esperar o filho fecha
    // também o fd do shell; drop/rename/fsync no pai não dariam essa garantia.
    let mut child = Command::new("/bin/sh")
        .args(["-c", "umask 077; exec 3>\"$1\" || exit; head -c \"$2\" >&3 && printf . && read -r release && chmod \"$3\" \"$1\"", "fixture"])
        .arg(path)
        .arg(contents.len().to_string())
        .arg(format!("{mode:o}"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Confirma a abertura no filho: permite reproduzir causalmente o fork
    // concorrente enquanto o arquivo está aberto, sem esperas por tempo.
    let mut input = child.stdin.take().unwrap();
    input.write_all(contents).unwrap();
    let mut ready = [0];
    child.stdout.take().unwrap().read_exact(&mut ready).unwrap();
    assert_eq!(ready, [b'.']);
    during_write();
    input.write_all(b"\n").unwrap();
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "gravar executável: {}", String::from_utf8_lossy(&output.stderr));
}

#[cfg(target_os = "linux")]
#[test]
fn fork_concorrente_reproduz_etxtbsy_antes_e_executa_depois() {
    use std::{
        fs::OpenOptions,
        os::{
            fd::AsRawFd,
            unix::{fs::OpenOptionsExt, net::UnixStream, process::CommandExt},
        },
        sync::mpsc,
        thread,
    };
    let root = std::env::temp_dir().join(format!("fixture-fork-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let (requests, incoming) = mpsc::channel::<(UnixStream, UnixStream)>();
    // Outra thread faz spawns reais em loop. pre_exec força fork e segura o
    // filho antes do exec (onde CLOEXEC fecharia o fd herdado). Só syscalls
    // async-signal-safe são usadas no filho, sem locks/alocação após fork.
    let spawner = thread::spawn(move || {
        for (ready, release) in incoming {
            let ready_fd = ready.as_raw_fd();
            let release_fd = release.as_raw_fd();
            let mut command = Command::new("/bin/true");
            // SAFETY: os sockets vivem até wait; o filho usa somente read/write
            // com ponteiro para um byte local válido, sem locks ou alocação.
            unsafe {
                command.pre_exec(move || {
                    let mut byte = 1_u8;
                    if libc::write(ready_fd, (&raw const byte).cast(), 1) != 1
                        || libc::read(release_fd, (&raw mut byte).cast(), 1) != 1
                    {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            assert!(command.spawn().unwrap().wait().unwrap().success());
        }
    });
    for fixed in [false, true] {
        for round in 0..4 {
            let path = root.join(format!("{fixed}-{round}"));
            let (mut ready, child_ready) = UnixStream::pair().unwrap();
            let (mut release, child_release) = UnixStream::pair().unwrap();
            let overlap = || {
                requests.send((child_ready, child_release)).unwrap();
                ready.read_exact(&mut [0]).unwrap();
            };
            if fixed {
                write_executable_during(&path, b"#!/bin/sh\nexit 0\n", 0o700, overlap);
            } else {
                // Mesmo inode publicado após fechar a escrita no pai: o fork
                // concorrente ainda mantém sua cópia do fd aberto.
                let temporary = path.with_extension("tmp");
                let mut file = OpenOptions::new().write(true).create_new(true).mode(0o700).open(&temporary).unwrap();
                file.write_all(b"#!/bin/sh\nexit 0\n").unwrap();
                overlap();
                drop(file);
                std::fs::rename(temporary, &path).unwrap();
            }
            let result = Command::new(&path).status();
            // Libera o filho mesmo se a asserção abaixo falhar.
            release.write_all(&[1]).unwrap();
            if fixed {
                assert!(result.unwrap().success(), "depois: {round}");
                println!("DEPOIS {round}: exec OK com fork concorrente");
            } else {
                assert_eq!(result.unwrap_err().raw_os_error(), Some(libc::ETXTBSY), "antes: {round}");
                println!("ANTES {round}: ETXTBSY após fechar fd no pai, filho mantém herança");
            }
        }
    }
    drop(requests);
    spawner.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
