volatile unsigned long identity(volatile unsigned long value) { volatile unsigned long local = value; return local; }
volatile unsigned long declared(volatile unsigned long value);
volatile unsigned long call_definition(volatile unsigned long value) { return identity(value); }
volatile unsigned long call_prototype(volatile unsigned long value) { return declared(value); }
