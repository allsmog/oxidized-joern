#define COUNT(x) (sizeof(x)/sizeof(char)-1)
int read(void) { return COUNT("abc"); }
