#define VALUE 1
int choose(int x) {
 int before=VALUE;
#undef VALUE
#define VALUE 2
 return before + VALUE + x;
}
