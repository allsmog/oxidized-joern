struct Item { int values[4]; int len; };
#define Field values[1]
int read(void *p) { return ((struct Item *)p)->Field; }
