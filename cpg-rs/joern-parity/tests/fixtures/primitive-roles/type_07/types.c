unsigned short identity(unsigned short value) { unsigned short local = value; return local; }
unsigned short declared(unsigned short value);
unsigned short call_definition(unsigned short value) { return identity(value); }
unsigned short call_prototype(unsigned short value) { return declared(value); }
