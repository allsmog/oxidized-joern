unsigned short int identity(unsigned short int value) { unsigned short int local = value; return local; }
unsigned short int declared(unsigned short int value);
unsigned short int call_definition(unsigned short int value) { return identity(value); }
unsigned short int call_prototype(unsigned short int value) { return declared(value); }
