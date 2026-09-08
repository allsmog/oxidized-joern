#define VALUE 1
int choose(int condition) {
 if (condition) {
#undef VALUE
#define VALUE 2
 } else {
#undef VALUE
#define VALUE 3
 }
 return VALUE;
}
int later(void) { return VALUE; }
