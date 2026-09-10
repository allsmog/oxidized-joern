struct Item { int len; int code; };
#define FIELD len
#define Len FIELD
#define READ(p) ((p)->Len)
int read(struct Item *p) { return READ(p); }
