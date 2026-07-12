VAR grid = 0
VAR other = 0

~ {
    grid = #[#[1, 2], #[3]]
    other = grid
    push(grid[1], 4)
    insert(grid[0], 0, 0)
}

Grid is {grid}. Other is {other}.
-> END
