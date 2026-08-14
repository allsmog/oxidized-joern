#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>

void fixed_cases(void *db, char *dst, size_t dst_size) {
    char *input = getenv("ATTACKER_INPUT");
    char *statement = 0;
    fgets(dst, (int)dst_size, stdin);
    snprintf(dst, dst_size, "%s", input);
    printf("%s", input);
    sqlite3_prepare_v2(db, "SELECT value FROM items WHERE id = ?", -1, &statement, 0);
    sqlite3_bind_text(statement, 1, input, -1, 0);
    fopen("/srv/allowlisted/data.txt", "r");
    dlopen("/usr/lib/liballowlisted.so", RTLD_NOW);
    char template[] = "/tmp/cpg.XXXXXX";
    int fd = mkstemp(template);
    scanf("%31s", dst);
    snprintf(dst, dst_size, "%s", "constant");
    if (fd >= 0) {
        close(fd);
    }
}
