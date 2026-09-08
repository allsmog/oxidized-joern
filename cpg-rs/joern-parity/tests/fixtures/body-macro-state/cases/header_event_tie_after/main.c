#define MODE 1
int before(void) { return MODE; }
void prepare(void) {
#undef MODE
#define MODE 3
}
#include "api.h"
int after(void) { return RESULT + MODE; }
