struct Item { int values[4]; int len; };
int index;
#define Len values[index]
int read(struct Item *p) { return p->Len; }
