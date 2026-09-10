struct Item { int values[4]; int len; };
#define Len values[index]
int read(struct Item *p, int index) { return p->Len; }
