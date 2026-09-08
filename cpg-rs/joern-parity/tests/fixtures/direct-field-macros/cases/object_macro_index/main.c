struct Item { int values[4]; int len; };
#define Index 1
#define Len values[Index]
int read(struct Item *p) { return p->Len; }
