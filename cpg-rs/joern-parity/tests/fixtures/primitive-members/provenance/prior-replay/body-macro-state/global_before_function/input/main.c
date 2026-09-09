#define PICK 1
int global = PICK;
#undef PICK
#define PICK 2
int choose(void) { return PICK; }
