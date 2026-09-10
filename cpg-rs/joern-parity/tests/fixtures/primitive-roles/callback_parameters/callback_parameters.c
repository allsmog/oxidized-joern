unsigned long callback_direct(unsigned long (*callback)(unsigned long), unsigned long value) { return callback(value); }
unsigned long callback_deref(unsigned long (*callback)(unsigned long), unsigned long value) { return (*callback)(value); }
unsigned long callback_paren(unsigned long (*callback)(unsigned long), unsigned long value) { return (callback)(value); }
