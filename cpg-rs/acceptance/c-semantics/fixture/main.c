#include <feature.h>

#if HEADER_ON && FORCE_ON && CLI_ON
int selected(int value) {
    return SCALE(value);
}
#else
int dead(void) {
    return 0;
}
#endif
