== computerdesk ==

    It's a desk with a very old personal computer on it. This kind still takes floppy disks.
    
    { 
     - computerdesk_first: -> computerdesk_repeat
     - else: -> computerdesk_first
    }
    
    -> END
    
== computerdesk_first ==

    &PORTRAIT_nour_nourpondering
    
    NOUR: "Here's a computer, but uh-"
    
    MS. SUWAN: "I expected something more... <i>operable</i>."
    
    &PORTRAIT_nick_nickworried
    
    NICK: "Does it even, like, <i>do stuff</i>?"
    
    NOUR: "There's no operating system loaded on it. Looks like it's missing a floppy disk."
    
    &PORTRAIT_nick_nickpondering
    
    NICK: "It's missing a <i>what</i>?"
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: "A small card you had to put into old computers to get them to work."
    
    NOUR: "It's like a CD, before those were used."
    
    MS. SUWAN: "I didn't think people your age knew about floppy disks."
    
    NOUR: "My family was into old tech. <i>Really</i> old tech."
    
    &PORTRAIT_nour_nourannoyed
    
    NOUR: "I didn't even get my first phone until high school."
    
    &PORTRAIT_nick_nickworried
    
    NICK: "Where do we get a <i>'floppy dip'</i>?"
    
    &PORTRAIT_ms. suwan_mssuwanannoyed
    
    MS. SUWAN: "A floppy <i>disk</i>. I do not believe there are any here, which is very strange."
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "<i>That's</i> very strange to you?"
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "...Well, this seems like a dead end if it won't help us get out of here."
    
    ->END
==computerdesk_repeat==

    &PORTRAIT_nour_nourannoyed
    NOUR: <i> Stupid computer doesn't work. I should find another way to get out of here. </i>
    -> END