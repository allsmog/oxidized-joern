#define PICK 1
int choose(void) {
#if PICK == 1
#undef PICK
#define PICK 2
#endif
 return PICK;
}
