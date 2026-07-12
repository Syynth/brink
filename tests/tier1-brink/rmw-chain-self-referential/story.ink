VAR grid = 0
VAR result = 0

~ {
    grid = #[#[1, 2, 3], #[4, 5, 6]]
    grid[0][0] = grid[0][1] + grid[1][2]
    grid[1][2] = grid[1][2] + grid[0][0]
    result = grid[0][0] + grid[1][2]
}

Result is {result}. Grid is {grid}.
-> END
