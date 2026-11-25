/* Filho de teste do transporte rc.2: binário real, executado pelo caminho do descritor
 * fixado pelo pai. O cenário é escolhido em tempo de compilação (-DCENARIO_*); sem -D vale
 * o cenário padrão. O protocolo é o mínimo acordado: uma linha JSON de bootstrap em stdout,
 * token recebido em stdin, HTTP só em 127.0.0.1 com Host e Bearer exatos, EOF em stdin pede
 * encerramento gracioso. Este arquivo é fixture de teste, não código do produto. */
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <netinet/in.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/select.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>

static char bearer[128];

/* Marcadores ao lado do próprio binário (dirname de /proc/self/exe): argv[0] é inútil no
 * exec por /proc/self/fd/<n> e o cwd é o do processo pai; cada prova copia o binário para
 * uma pasta exclusiva, então o marcador cai no lugar certo. */
static void grava_marcador(const char *nome, int modo_signal_safe) {
  char exe[4096];
  ssize_t n = readlink("/proc/self/exe", exe, sizeof exe - 1);
  if (n <= 0) return;
  exe[n] = '\0';
  char *barra = strrchr(exe, '/');
  if (barra == NULL) return;
  char caminho[4096];
  int prefixo = snprintf(caminho, sizeof caminho, "%.*s/%s", (int)(barra - exe), exe, nome);
  if (prefixo <= 0 || (size_t)prefixo >= sizeof caminho) return;
  int fd = open(caminho, O_WRONLY | O_CREAT | O_TRUNC, 0600);
  if (fd < 0) return;
  static const char conteudo[] = "presente\n";
  ssize_t ignorado = write(fd, conteudo, sizeof conteudo - 1);
  (void)ignorado;
  close(fd);
  (void)modo_signal_safe;
}

/* Caminho do marcador do sinal pré-computado ANTES de instalar o handler: dentro do
 * handler só entram syscalls async-signal-safe (open/write/close/_exit) — nada de snprintf,
 * stdio ou alocação, que a glibc não garante em contexto de sinal. */
static char caminho_marcador_sinal[4096];

static int prepara_marcador_sinal(void) {
  char exe[4096];
  ssize_t n = readlink("/proc/self/exe", exe, sizeof exe - 1);
  if (n <= 0) return -1;
  exe[n] = '\0';
  char *barra = strrchr(exe, '/');
  if (barra == NULL) return -1;
  int prefixo = snprintf(caminho_marcador_sinal, sizeof caminho_marcador_sinal, "%.*s/sigterm.marcador", (int)(barra - exe), exe);
  if (prefixo <= 0 || (size_t)prefixo >= sizeof caminho_marcador_sinal) return -1;
  return 0;
}

static void ao_sigterm(int sinal) {
  (void)sinal;
  int fd = open(caminho_marcador_sinal, O_WRONLY | O_CREAT | O_TRUNC, 0600);
  if (fd >= 0) {
    static const char conteudo[] = "presente\n";
    ssize_t ignorado = write(fd, conteudo, sizeof conteudo - 1);
    (void)ignorado;
    close(fd);
  }
  _exit(0);
}

static void __attribute__((unused)) espera_eof(void) {
  char memoria[64];
  for (;;) {
    ssize_t n = read(STDIN_FILENO, memoria, sizeof memoria);
    if (n == 0) exit(0);
    if (n < 0 && errno != EINTR) exit(1);
  }
}

static int abre_escuta(void) {
  int fd = socket(AF_INET, SOCK_STREAM, 0);
  if (fd < 0) exit(1);
  struct sockaddr_in addr;
  memset(&addr, 0, sizeof addr);
  addr.sin_family = AF_INET;
  addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
  addr.sin_port = 0;
  if (bind(fd, (struct sockaddr *)&addr, sizeof addr) < 0) exit(1);
  if (listen(fd, 8) < 0) exit(1);
  return fd;
}

static int porta_da_escuta(int fd) {
  struct sockaddr_in addr;
  socklen_t tamanho = sizeof addr;
  if (getsockname(fd, (struct sockaddr *)&addr, &tamanho) < 0) exit(1);
  return ntohs(addr.sin_port);
}

static void escreve_linha_bootstrap(int porta) {
  printf("{\"v\":1,\"port\":%d}\n", porta);
  fflush(stdout);
}

static void le_token(void) {
  char linha[512];
  size_t usados = 0;
  for (;;) {
    char c;
    ssize_t n = read(STDIN_FILENO, &c, 1);
    if (n < 0) { if (errno == EINTR) continue; exit(1); }
    if (n == 0) exit(0);
    if (c == '\n') break;
    if (usados < sizeof linha - 1) linha[usados++] = c;
  }
  linha[usados] = '\0';
  const char *alvo = strstr(linha, "\"bearer\":\"");
  if (alvo == NULL) exit(1);
  alvo += strlen("\"bearer\":\"");
  size_t i = 0;
  while (alvo[i] != '\0' && alvo[i] != '"' && i < sizeof bearer - 1) {
    bearer[i] = alvo[i];
    i++;
  }
  bearer[i] = '\0';
  if (i == 0) exit(1);
}

static void atende(int fd, int porta, int rejeita_bearer) {
  char req[8192];
  size_t usados = 0;
  for (;;) {
    if (usados >= sizeof req - 1) { close(fd); return; }
    ssize_t n = recv(fd, req + usados, sizeof req - 1 - usados, 0);
    if (n < 0) { if (errno == EINTR) continue; close(fd); return; }
    if (n == 0) { close(fd); return; }
    usados += (size_t)n;
    if (usados >= 4 && memcmp(req + usados - 4, "\r\n\r\n", 4) == 0) break;
  }
  req[usados] = '\0';
  char host[64], autorizacao[192];
  snprintf(host, sizeof host, "\r\nHost: 127.0.0.1:%d\r\n", porta);
  snprintf(autorizacao, sizeof autorizacao, "\r\nAuthorization: Bearer %s\r\n", bearer);
  int pedido_conhecido = strncmp(req, "GET /hello ", 11) == 0 || strncmp(req, "GET /events ", 12) == 0;
  if (rejeita_bearer || !pedido_conhecido || strstr(req, host) == NULL || strstr(req, autorizacao) == NULL) {
    static const char recusa[] = "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    send(fd, recusa, sizeof recusa - 1, 0);
    close(fd);
    return;
  }
  if (strncmp(req, "GET /hello ", 11) == 0) {
    static const char corpo[] = "{\"computerId\":\"filho-teste\",\"hostId\":\"filho-hub\"}";
    char cabecalho[256];
    int n = snprintf(cabecalho, sizeof cabecalho,
      "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: %zu\r\nConnection: close\r\n\r\n%s",
      sizeof corpo - 1, corpo);
    send(fd, cabecalho, (size_t)n, 0);
    close(fd);
    return;
  }
  static const char eventos[] =
    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: keep-alive\r\n\r\n"
    "data: {\"type\":\"batida\"}\n\n";
  send(fd, eventos, sizeof eventos - 1, 0);
  /* Mantém a conexão aberta: o pai fecha ao destruir o soquete. */
}

static void serve(int escuta, int porta, int rejeita_bearer, int sai_no_eof) {
  int vigia_stdin = 1;
  for (;;) {
    fd_set conjuntos;
    FD_ZERO(&conjuntos);
    FD_SET(escuta, &conjuntos);
    if (vigia_stdin) FD_SET(STDIN_FILENO, &conjuntos);
    int maior = escuta > STDIN_FILENO ? escuta : STDIN_FILENO;
    if (select(maior + 1, &conjuntos, NULL, NULL, NULL) < 0) { if (errno == EINTR) continue; exit(1); }
    if (vigia_stdin && FD_ISSET(STDIN_FILENO, &conjuntos)) {
      char c;
      ssize_t n = read(STDIN_FILENO, &c, 1);
      if (n == 0) {
        if (sai_no_eof) { grava_marcador("eof-gracioso.marcador", 0); fprintf(stderr, "eof-gracioso\n"); exit(0); }
        vigia_stdin = 0;
      } else if (n < 0 && errno != EINTR) {
        exit(1);
      }
    }
    if (FD_ISSET(escuta, &conjuntos)) {
      int fd = accept(escuta, NULL, NULL);
      if (fd >= 0) atende(fd, porta, rejeita_bearer);
    }
  }
}

int main(void) {
#if defined(CENARIO_LINHA_INVALIDA)
  printf("isto não é JSON\n");
  fflush(stdout);
  espera_eof();
#elif defined(CENARIO_LINHA_EXTRA)
  /* Linha extra no mesmo flush: focal da dupla de guardas de byte além da linha válida. */
  { int fd = abre_escuta();
    int porta = porta_da_escuta(fd);
    printf("{\"v\":1,\"port\":%d}\nlinha extra\n", porta);
    fflush(stdout);
    le_token();
    serve(fd, porta, 0, 1); }
#elif defined(CENARIO_LINHA_ATRASADA)
  /* Dados sem quebra em chunk separado: isola a guarda de dados que chegam depois da linha válida. */
  { int fd = abre_escuta();
    int porta = porta_da_escuta(fd);
    escreve_linha_bootstrap(porta);
    usleep(300000);
    printf("lixo atrasado sem quebra");
    fflush(stdout);
    le_token();
    serve(fd, porta, 0, 1); }
#elif defined(CENARIO_PORTA_BAIXA)
  escreve_linha_bootstrap(80);
  espera_eof();
#elif defined(CENARIO_NAO_ESCUTA)
  escreve_linha_bootstrap(45678);
  espera_eof();
#elif defined(CENARIO_BEARER_REJEITA)
  { int fd = abre_escuta();
    int porta = porta_da_escuta(fd);
    escreve_linha_bootstrap(porta);
    le_token();
    serve(fd, porta, 1, 1); }
#elif defined(CENARIO_MUDO)
  { int fd = abre_escuta();
    escreve_linha_bootstrap(porta_da_escuta(fd));
    le_token();
    int cliente = accept(fd, NULL, NULL);
    (void)cliente;
    for (;;) pause(); }
#elif defined(CENARIO_IGNORA_SIGTERM)
  signal(SIGTERM, SIG_IGN);
  { int fd = abre_escuta();
    int porta = porta_da_escuta(fd);
    escreve_linha_bootstrap(porta);
    le_token();
    serve(fd, porta, 0, 0); }
#elif defined(CENARIO_DESCENDENTE)
  /* SIGCHLD ignorado faz o kernel descartar o descendente morto por sinal: sem zumbi, a
   * sonda do pai confere grupo vazio por ESRCH em qualquer ambiente, sem depender do init. */
  signal(SIGCHLD, SIG_IGN);
  { int fd = abre_escuta();
    int porta = porta_da_escuta(fd);
    escreve_linha_bootstrap(porta);
    le_token();
    pid_t pid = fork();
    if (pid == 0) { close(STDIN_FILENO); close(STDOUT_FILENO); for (;;) pause(); }
    serve(fd, porta, 0, 1); }
#elif defined(CENARIO_SEM_LINHA)
  /* Nunca escreve a linha de bootstrap e não lê stdin: o EOF do escalonamento é inútil,
   * a morte tem de vir do sinal — o handler grava a evidência do SIGTERM com syscalls
   * async-signal-safe sobre um caminho pré-computado antes da instalação. */
  if (prepara_marcador_sinal() != 0) exit(1);
  signal(SIGTERM, ao_sigterm);
  for (;;) pause();
#else
  { int fd = abre_escuta();
    int porta = porta_da_escuta(fd);
    escreve_linha_bootstrap(porta);
    le_token();
    serve(fd, porta, 0, 1); }
#endif
  return 0;
}
