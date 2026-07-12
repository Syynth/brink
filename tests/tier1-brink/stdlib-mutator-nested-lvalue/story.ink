VAR grid = 0

~ {
    grid = #[#[1, 2], #[3]]
    push(grid[1], 4)
    insert(grid[0], 0, 0)
}

Grid is {grid}.
-> END
