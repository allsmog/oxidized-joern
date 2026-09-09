#define PICK 1
int first(void) { return PICK; }
#undef PICK
#define PICK 2L
long global = PICK;
#undef PICK
#define PICK 3LL
long long later(void) { return PICK; }
