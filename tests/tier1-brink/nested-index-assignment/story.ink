VAR grid = 0
VAR result = 0

~ {
    grid = #[#[1, 2], #[3, 4]]
    grid[0][1] = 99
    grid[1][0] = grid[1][0] + 100
    result = grid[0][1] + grid[1][0]
}

Result is {result}.
-> END
