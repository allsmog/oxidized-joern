#define PICK 1
const char *saved = "PICK";
#undef PICK
#define PICK 2
int choose(void) { return PICK; }
