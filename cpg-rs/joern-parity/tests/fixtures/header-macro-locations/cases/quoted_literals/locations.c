#include "defs.h"
char *probe(void) { return ID("a b"); }
char *escaped(void) { return ID("a\tb"); }
int character(void) { return ID(' '); }
