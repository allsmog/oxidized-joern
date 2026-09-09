#define PICK 1
int choose(void) {
 int first = PICK;
#undef PICK
#define PICK 2
 return first + PICK;
}
