/* A load generator for tools/serve_bench.py: requests a second from an HTTP server that answers one
 * request per connection (AIPL's std/serve and the hosts of `serve.once` close after each answer).
 *
 *     loadgen HOST PORT PATH TOTAL CONCURRENCY
 *
 * CONCURRENCY threads each loop: connect, send `GET PATH` with `Connection: close`, read to the end,
 * count the response if it starts with "HTTP/1.x 200". Prints one line:
 *
 *     ok=N errors=N seconds=S rps=R p50_ms=X p99_ms=Y
 *
 * A connection refused is retried for up to 30 seconds (the server may be starting) and, until one
 * succeeds, nothing is counted: the time starts at the first answer. Build: gcc -O2 -pthread.
 */
#include <arpa/inet.h>
#include <errno.h>
#include <netinet/in.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

static struct sockaddr_in addr;
static char request[512];
static size_t request_len;
static long total;
static long next_request;
static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;
static long ok, errors;
static double *latency;

static double now(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec + t.tv_nsec * 1e-9;
}

/* One exchange; returns the seconds it took, or a negative number on a failure. */
static double exchange(int *good) {
    double start = now();
    int fd = -1;
    for (int tries = 0; tries < 3000; tries++) {
        fd = socket(AF_INET, SOCK_STREAM, 0);
        if (fd < 0) return -1;
        if (connect(fd, (struct sockaddr *)&addr, sizeof addr) == 0) break;
        close(fd);
        fd = -1;
        if (errno != ECONNREFUSED) return -1;
        usleep(10000);
    }
    if (fd < 0) return -1;
    start = now();
    size_t sent = 0;
    while (sent < request_len) {
        ssize_t n = send(fd, request + sent, request_len - sent, MSG_NOSIGNAL);
        if (n <= 0) { close(fd); return -1; }
        sent += n;
    }
    char head[16] = {0};
    size_t have = 0;
    char buf[8192];
    for (;;) {
        ssize_t n = read(fd, buf, sizeof buf);
        if (n < 0) { close(fd); return -1; }
        if (n == 0) break;
        if (have < 12) {
            size_t take = (size_t)n < 12 - have ? (size_t)n : 12 - have;
            memcpy(head + have, buf, take);
            have += take;
        }
    }
    close(fd);
    *good = have >= 12 && memcmp(head, "HTTP/1.", 7) == 0 && memcmp(head + 8, " 200", 4) == 0;
    return now() - start;
}

static void *worker(void *arg) {
    (void)arg;
    for (;;) {
        pthread_mutex_lock(&lock);
        long k = next_request < total ? next_request++ : -1;
        pthread_mutex_unlock(&lock);
        if (k < 0) return NULL;
        int good = 0;
        double took = exchange(&good);
        pthread_mutex_lock(&lock);
        if (took >= 0 && good) { latency[ok] = took; ok++; } else errors++;
        pthread_mutex_unlock(&lock);
    }
}

static int compare(const void *a, const void *b) {
    double x = *(const double *)a, y = *(const double *)b;
    return (x > y) - (x < y);
}

int main(int argc, char **argv) {
    if (argc != 6) { fprintf(stderr, "usage: loadgen HOST PORT PATH TOTAL CONCURRENCY\n"); return 2; }
    memset(&addr, 0, sizeof addr);
    addr.sin_family = AF_INET;
    addr.sin_port = htons((unsigned short)atoi(argv[2]));
    if (inet_pton(AF_INET, argv[1], &addr.sin_addr) != 1) { fprintf(stderr, "loadgen: HOST must be an IPv4 address\n"); return 2; }
    request_len = (size_t)snprintf(request, sizeof request, "GET %s HTTP/1.1\r\nHost: %s:%s\r\nConnection: close\r\n\r\n", argv[3], argv[1], argv[2]);
    total = atol(argv[4]);
    int threads = atoi(argv[5]);
    latency = malloc(sizeof(double) * (size_t)total);
    pthread_t t[256];
    if (threads > 256) threads = 256;
    /* the first answer starts the clock, so a server still starting does not count against it */
    int good = 0;
    double began = now();
    while (exchange(&good) < 0 || !good) {
        if (now() - began > 30) { fprintf(stderr, "loadgen: no answer from the server in 30 seconds\n"); return 1; }
        usleep(20000);
    }
    double start = now();
    next_request = 1; /* the one just made is the first */
    ok = 1; latency[0] = 0;
    for (int i = 0; i < threads; i++) pthread_create(&t[i], NULL, worker, NULL);
    for (int i = 0; i < threads; i++) pthread_join(t[i], NULL);
    double seconds = now() - start;
    qsort(latency + 1, (size_t)(ok - 1), sizeof(double), compare);
    long n = ok - 1;
    double p50 = n > 0 ? latency[1 + n / 2] * 1e3 : 0, p99 = n > 0 ? latency[1 + (n * 99) / 100 - (n * 99 / 100 == n)] * 1e3 : 0;
    printf("ok=%ld errors=%ld seconds=%.3f rps=%.0f p50_ms=%.3f p99_ms=%.3f\n", ok - 1, errors, seconds, seconds > 0 ? (double)(ok - 1) / seconds : 0, p50, p99);
    return errors > 0;
}
