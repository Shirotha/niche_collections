// use tree to store variable sized freed memory regions
// each leaf stores a single free section
// each branch stores two free sections separated by used section
// use start of section as key
// use size of biggest continues section as data
//   (this will always be the size of the section itself on leafs)
// TODO: can the nodes be stored in the buffer itself?
//   (would need size for parent, child l/r, start, end (5x u32 = 20 B (align 4)))
// on delete:
//   - find node that would parent the new free section
//   - if both adjacent leaf nodes and new section would create single region:
//     - replace parent node with merged node
//   - otherwise insert new node here
// on insert:
//   - find smallest node bigger then requested memory
//   - if availible remove needed space (leaving smaller node behind) or remove node on exact match
//   - else need to append to buffer
