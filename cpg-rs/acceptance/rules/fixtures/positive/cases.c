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
}
