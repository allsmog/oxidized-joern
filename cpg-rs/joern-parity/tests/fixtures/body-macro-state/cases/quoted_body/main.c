#define PICK 1
const char *prefix(void) { return "PICK"; }
#undef PICK
#define PICK 2
int choose(void) { return PICK; }
