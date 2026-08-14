#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

void my_gets(char *dst);
void my_tmpnam(char *dst);
void my_scanf(const char *format, char *dst);
void my_sprintf(char *dst, const char *format, const char *value);

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
}
