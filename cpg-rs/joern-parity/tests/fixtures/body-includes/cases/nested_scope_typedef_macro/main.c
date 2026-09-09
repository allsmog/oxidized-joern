int Local(int x);
int choose(int x) {
 {
#include "scope.h"
  x = (Local)(x) + inside;
 }
 return Local(x) + ESCAPED;
}
int later(int x) { return Local(x) + ESCAPED; }
