#define PICK 1
#if PICK == 1
#include "api.h"
#endif
int consume(const char *value);
int choose(void) {
 /* Preserve ordinary source text: api.h:PICK:int(0). */
 consume("api.h:PICK:int(0)");
 return PICK;
}
