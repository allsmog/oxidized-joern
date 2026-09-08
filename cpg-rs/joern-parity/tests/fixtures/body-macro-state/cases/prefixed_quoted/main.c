#define PICK 1
void prefix(void) { L"PICK"; u8"PICK"; }
#undef PICK
#define PICK 2
int choose(void) { return PICK; }
