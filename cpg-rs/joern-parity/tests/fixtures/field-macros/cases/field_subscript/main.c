struct Item { int values[2]; };
#define Len values[1]
#define READ(p) ((p)->Len)
int read(struct Item *p) { return READ(p); }
