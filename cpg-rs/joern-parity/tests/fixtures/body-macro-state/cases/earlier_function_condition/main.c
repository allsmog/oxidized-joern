#define PICK 1
int earlier(void) {
#if PICK == 1
#undef PICK
#define PICK 2
#endif
 return 0;
}
int later(void) { return PICK; }
