#define PICK 1
#if PICK == 1
#include "api header.h"
#endif
int consume(const char *value);
int choose(void) {
 /* Preserve ordinary source text: api header.h:PICK:int(0). */
 consume("api header.h:PICK:int(0)");
 return PICK;
}
