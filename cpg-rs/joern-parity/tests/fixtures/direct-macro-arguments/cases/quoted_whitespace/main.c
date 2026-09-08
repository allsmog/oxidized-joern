#define ID(x) (x)
const char *quoted(void) { return ID("a b\" c"); }
int spaces(int a) { return ID(( a 	 +  1 )); }
