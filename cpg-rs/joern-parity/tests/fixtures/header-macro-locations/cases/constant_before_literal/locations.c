#include "defs.h"
void sink(int value);
void probe(void) {
  int value = SEVENTEEN;
  sink(
      17
  );
  value = 17;
  sink(value);
}
