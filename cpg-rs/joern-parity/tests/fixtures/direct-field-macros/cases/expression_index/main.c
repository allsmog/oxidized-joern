struct Item { int values[4]; int len; };
#define Len values[index + 1]
int read(struct Item *p, int index) { return p->Len; }
