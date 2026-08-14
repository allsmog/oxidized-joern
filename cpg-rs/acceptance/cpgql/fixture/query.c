#include <stdlib.h>
#include <string.h>

int helper(int value) {
    return value + 1;
}

int main(int argc, char **argv) {
    char destination[32];
    char *input = getenv("ATTACKER_INPUT");
    if (argc > 1) {
        strcpy(destination, input);
    }
    return helper(argc);
}
