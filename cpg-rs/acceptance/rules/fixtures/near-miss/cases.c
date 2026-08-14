#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

void my_gets(char *dst);

void near_miss_cases(void *db, char *dst) {
    char *input = getenv("ATTACKER_INPUT");
    system("/usr/bin/id");
    strcpy(dst, "constant");
    printf("%s", input);
    my_gets(dst);
    sqlite3_exec(db, "SELECT 1", 0, 0, 0);
    fopen("/etc/hosts", "r");
    dlopen("libsafe.so", RTLD_NOW);
}
