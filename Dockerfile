# Use the official Bun image as a base
FROM oven/bun:latest

# Set the working directory inside the container
WORKDIR /app

# Install Ghostscript
# Update package lists and install ghostscript
RUN apt-get update && apt-get install -y ghostscript

# Copy package.json and bun.lockb to leverage Docker cache
# This step ensures that bun install is only re-run if dependencies change
COPY package.json bun.lockb ./

# Install Bun dependencies
RUN bun install --production

# Copy the rest of your application code
COPY . .

# Expose the port your application listens on
# Your server listens on process.env.PORT or 9001
EXPOSE 9001

# Command to run the application
# Use 'bun run start' as defined in your package.json
CMD ["bun", "run", "start"]
