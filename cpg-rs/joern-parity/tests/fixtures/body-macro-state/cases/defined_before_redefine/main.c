#define PICK 1
int choose(void) {
#if defined(PICK)
#undef PICK
#define PICK 2
#endif
 return PICK;
}
