#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

void positive_cases(void *db, char *dst) {
    char *input = getenv("ATTACKER_INPUT");
    system(input);
    strcpy(dst, input);
    printf(input);
    gets(dst);
    sqlite3_exec(db, input, 0, 0, 0);
    fopen(input, "r");
    dlopen(input, RTLD_NOW);
    tmpnam(dst);
    scanf("%s", dst);
    sprintf(dst, "%s", "constant");
}

void expanded_positive_cases(int fd, char *dst, const char *src, void *digest) {
    memcpy(dst, src, getenv("ATTACKER_COUNT"));
    void *allocation = malloc(getenv("ATTACKER_COUNT"));

    char *host = getenv("ATTACKER_HOST");
    getaddrinfo(host, "443", 0, 0);

    recv(fd, dst, 64, 0);
    rand();
    MD5(src, 1, digest);
    getwd(dst);
    free(allocation);
}
