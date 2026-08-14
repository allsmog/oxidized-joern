#include <stdlib.h>

char *external_input(void) {
    return getenv("ATTACKER_INPUT");
}
