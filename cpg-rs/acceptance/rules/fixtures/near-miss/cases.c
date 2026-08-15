#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

void my_gets(char *dst);
void my_tmpnam(char *dst);
void my_scanf(const char *format, char *dst);
void my_sprintf(char *dst, const char *format, const char *value);
void my_recv(int fd, char *dst, int size, int flags);
void my_rand(void);
void my_MD5(const char *src, int size, void *digest);
void my_getwd(char *dst);

void near_miss_cases(void *db, char *dst) {
    char *input = getenv("ATTACKER_INPUT");
    system("/usr/bin/id");
    strcpy(dst, "constant");
    printf("%s", input);
    my_gets(dst);
    sqlite3_exec(db, "SELECT 1", 0, 0, 0);
    fopen("/etc/hosts", "r");
    dlopen("libsafe.so", RTLD_NOW);
    my_tmpnam(dst);
    my_scanf("%s", dst);
    my_sprintf(dst, "%s", input);

    memcpy(input, dst, 16);
    void *allocation = malloc(16);
    getaddrinfo("localhost", "443", 0, 0);
    my_recv(0, dst, 16, 0);
    my_rand();
    my_MD5(input, 1, 0);
    my_getwd(dst);
    free(allocation);
}
