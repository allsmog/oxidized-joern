struct Item { int values[4]; int len; };
#define PTR(p) (p)
int read(struct Item *p) { return PTR(p)->len; }
