#define PICK 1
#define OTHER 8
int choose(void) {
#if PICK == 1
#undef PICK
#define PICK 2
#endif
 int x = OTHER;
 return PICK + x;
}
