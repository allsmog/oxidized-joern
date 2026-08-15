#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

char *external_input(void);

void multi_file_cases(void *db, char *dst) {
    char *input = external_input();
    system(input);
    strcpy(dst, input);
    printf(input);
    gets(dst);
    sqlite3_exec(db, input, 0, 0, 0);
    fopen(input, "r");
    dlopen(input, RTLD_NOW);
    tmpnam(dst);
    fscanf(stdin, "%[a-z]", dst);
    vsprintf(dst, "%s", "constant");
}

void expanded_multi_file_cases(int fd, char *dst, const char *src, void *digest) {
    char *input = external_input();
    memcpy(dst, src, input);
    void *allocation = malloc(input);
    getaddrinfo(input, "443", 0, 0);
    recv(fd, dst, 64, 0);
    random();
    SHA1(src, 1, digest);
    strtok(dst, ":");
    free(allocation);
}
